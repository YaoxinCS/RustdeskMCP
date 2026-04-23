use crate::{
    flutter::{
        self, AgentBridgeDisplaySnapshot, AgentBridgeFrameSnapshot, AgentBridgeSessionSnapshot,
        FlutterSession,
    },
    flutter_ffi::{self, SessionID},
    input::{
        MOUSE_BUTTON_LEFT, MOUSE_BUTTON_RIGHT, MOUSE_BUTTON_WHEEL, MOUSE_TYPE_DOWN,
        MOUSE_TYPE_MOVE, MOUSE_TYPE_UP,
    },
};
use encoding_rs::GBK;
use hbb_common::{
    base64::{engine::general_purpose::STANDARD, Engine as _},
    config::LocalConfig,
    log,
    message_proto::{terminal_response, ControlKey, KeyEvent, KeyboardMode, TerminalResponse},
    rendezvous_proto::ConnType,
};
use serde_json::{json, Map, Value};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    io::{self, BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    str::FromStr,
    sync::{Mutex, Once, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MCP_ENABLE_OPTION: &str = "enable-mcp-server";
const MCP_ENABLE_OPTION_LEGACY: &str = "enable-mcp-agent";
const MCP_LISTEN_ADDR: &str = "127.0.0.1:59940";
const MCP_HTTP_PATH: &str = "/mcp";
const MCP_SERVER_NAME: &str = "rustdesk";
const MCP_SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
const MCP_PROTOCOL_VERSION: &str = "2025-11-25";
const SESSION_OPEN_TIMEOUT: Duration = Duration::from_secs(12);
const SESSION_READY_TIMEOUT: Duration = Duration::from_secs(12);
const FRAME_WAIT_TIMEOUT: Duration = Duration::from_secs(3);

static START_MCP_SERVER: Once = Once::new();
static DESKTOP_SESSION_REGISTRY: OnceLock<Mutex<HashMap<String, DesktopSessionHandle>>> =
    OnceLock::new();
static FRAME_CACHE: OnceLock<Mutex<HashMap<(SessionID, usize), CachedFrame>>> = OnceLock::new();
static LOCKED_REMOTE_INPUT_PEERS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
static TERMINAL_CACHE: OnceLock<Mutex<HashMap<(SessionID, i32), String>>> = OnceLock::new();

thread_local! {
    static AGENT_INPUT_BYPASS: Cell<u32> = const { Cell::new(0) };
}

pub fn start_server_once() {
    START_MCP_SERVER.call_once(|| {
        let _ = std::thread::Builder::new()
            .name("rustdesk-mcp-http".to_string())
            .spawn(|| {
                if let Err(error) = serve_http() {
                    log::error!("RustDesk MCP HTTP server stopped: {error}");
                }
            });
    });
}

fn serve_http() -> io::Result<()> {
    let listener = TcpListener::bind(MCP_LISTEN_ADDR)?;
    log::info!("RustDesk MCP HTTP listening on http://{MCP_LISTEN_ADDR}{MCP_HTTP_PATH}");
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let _ = std::thread::Builder::new()
                    .name("rustdesk-mcp-http-client".to_string())
                    .spawn(move || {
                        if let Err(error) = handle_http_client(stream) {
                            log::warn!("RustDesk MCP HTTP client ended with error: {error}");
                        }
                    });
            }
            Err(error) => log::warn!("RustDesk MCP accept failed: {error}"),
        }
    }
    Ok(())
}

fn handle_http_client(mut stream: TcpStream) -> io::Result<()> {
    let request = match read_http_request(&stream)? {
        Some(request) => request,
        None => return Ok(()),
    };
    let response = route_http_request(request);
    write_http_response(&mut stream, response)
}

fn route_http_request(request: HttpRequest) -> HttpResponse {
    let path = request.path.split('?').next().unwrap_or_default();
    if request.method == "OPTIONS" {
        return HttpResponse::empty(204, "No Content");
    }
    if path != "/" && path != MCP_HTTP_PATH {
        return HttpResponse::text(404, "Not Found", "RustDesk MCP endpoint not found");
    }
    match request.method.as_str() {
        "GET" => HttpResponse::text(
            405,
            "Method Not Allowed",
            "Use POST on /mcp to access the RustDesk MCP server",
        ),
        "POST" => handle_mcp_http_post(request),
        _ => HttpResponse::text(405, "Method Not Allowed", "Unsupported HTTP method"),
    }
}

fn handle_mcp_http_post(request: HttpRequest) -> HttpResponse {
    let message = match serde_json::from_slice::<Value>(&request.body) {
        Ok(message) => message,
        Err(error) => {
            return HttpResponse::json(
                400,
                "Bad Request",
                jsonrpc_error(Value::Null, -32700, format!("parse error: {error}")),
            );
        }
    };
    if !mcp_enabled() {
        let id = message.get("id").cloned().unwrap_or(Value::Null);
        return HttpResponse::json(
            503,
            "Service Unavailable",
            jsonrpc_error(
                id,
                -32000,
                "MCP server is disabled. Enable it in Settings -> Security -> Enable MCP server.",
            ),
        );
    }
    match handle_message(message) {
        Some(response) => HttpResponse::json(200, "OK", response),
        None => HttpResponse::empty(202, "Accepted"),
    }
}

fn handle_message(message: Value) -> Option<Value> {
    let id = message.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        return Some(jsonrpc_error(id, -32600, "missing method"));
    };

    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": MCP_PROTOCOL_VERSION,
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": MCP_SERVER_NAME, "version": MCP_SERVER_VERSION }
        })),
        "initialized" | "notifications/initialized" => return None,
        "ping" => Ok(json!({})),
        "resources/list" => Ok(json!({ "resources": [] })),
        "prompts/list" => Ok(json!({ "prompts": [] })),
        "tools/list" => Ok(tools_list()),
        "tools/call" => handle_tool_call(message.get("params")),
        "shutdown" => Ok(json!({})),
        _ => Err(ProtocolError::new(
            -32601,
            format!("unsupported method '{method}'"),
        )),
    };

    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(error) => jsonrpc_error(id, error.code, error.message),
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            tool_def(
                "open_desktop_session",
                "Open a RustDesk desktop session through the normal user connection path.",
                json!({
                    "type": "object",
                    "properties": { "id": { "type": "string" } },
                    "required": ["id"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "open_terminal_session",
                "Open a RustDesk terminal session and create terminal id 1.",
                json!({
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "rows": { "type": "integer", "minimum": 1 },
                        "cols": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "terminal_input",
                "Send text to an open RustDesk terminal session.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "terminalId": { "type": "integer" },
                        "data": { "type": "string" }
                    },
                    "required": ["session", "terminalId", "data"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "terminal_output",
                "Read cached output from an open RustDesk terminal session.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "terminalId": { "type": "integer" },
                        "limit": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["session", "terminalId"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "lock_remote_user_input",
                "Block the remote user's local keyboard and mouse input for the current desktop session.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" }
                    },
                    "required": ["session"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "unlock_remote_user_input",
                "Restore the remote user's local keyboard and mouse input for the current desktop session.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" }
                    },
                    "required": ["session"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "select_desktop_display",
                "Switch the current desktop session to a specific remote display index.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "display": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["session", "display"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "input_password",
                "Provide the RustDesk connection password for a pending desktop session.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "password": { "type": "string" }
                    },
                    "required": ["session", "password"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "get_desktop_frame",
                "Return one desktop frame as a PNG image for the selected display of a RustDesk desktop session.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "display": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["session"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "mouse_move",
                "Move the remote mouse for a desktop session while temporarily blocking user input.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "x": { "type": "integer", "minimum": 0 },
                        "y": { "type": "integer", "minimum": 0 }
                    },
                    "required": ["session", "x", "y"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "mouse_click",
                "Click the remote mouse for a desktop session while temporarily blocking user input.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "x": { "type": "integer", "minimum": 0 },
                        "y": { "type": "integer", "minimum": 0 },
                        "button": { "type": "string" },
                        "clicks": { "type": "integer", "minimum": 1 }
                    },
                    "required": ["session", "x", "y"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "keyboard_input",
                "Send text to the remote keyboard for a desktop session while temporarily blocking user input.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "text": { "type": "string" }
                    },
                    "required": ["session", "text"],
                    "additionalProperties": false
                })
            ),
            tool_def(
                "keyboard_hotkey",
                "Send a key chord such as Ctrl+V or Enter to the remote desktop session while temporarily blocking user input.",
                json!({
                    "type": "object",
                    "properties": {
                        "session": { "type": "string" },
                        "keys": {
                            "type": "array",
                            "items": { "type": "string" },
                            "minItems": 1
                        }
                    },
                    "required": ["session", "keys"],
                    "additionalProperties": false
                })
            )
        ]
    })
}

fn tool_def(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

fn handle_tool_call(params: Option<&Value>) -> Result<Value, ProtocolError> {
    let params = params
        .and_then(Value::as_object)
        .ok_or_else(|| ProtocolError::new(-32602, "tools/call params must be an object"))?;
    let name = required_str(params, "name")?;
    let empty = Map::new();
    let args = params
        .get("arguments")
        .and_then(Value::as_object)
        .unwrap_or(&empty);

    Ok(match name {
        "open_desktop_session" => tool_payload_result(open_desktop_session(required_str(args, "id")?)),
        "open_terminal_session" => tool_payload_result(open_terminal_session(
            required_str(args, "id")?,
            optional_u32(args, "rows")?.unwrap_or(30),
            optional_u32(args, "cols")?.unwrap_or(120),
        )),
        "terminal_input" => tool_payload_result(terminal_input(
            required_str(args, "session")?,
            required_i32(args, "terminalId")?,
            required_str(args, "data")?,
        )),
        "terminal_output" => tool_payload_result(terminal_output(
            required_str(args, "session")?,
            required_i32(args, "terminalId")?,
            optional_u32(args, "limit")?.unwrap_or(8192) as usize,
        )),
        "lock_remote_user_input" => tool_payload_result(lock_remote_user_input(required_str(args, "session")?)),
        "unlock_remote_user_input" => tool_payload_result(unlock_remote_user_input(required_str(args, "session")?)),
        "select_desktop_display" => tool_payload_result(select_desktop_display(
            required_str(args, "session")?,
            required_u32(args, "display")? as usize,
        )),
        "input_password" => tool_payload_result(input_password(
            required_str(args, "session")?,
            required_str(args, "password")?,
        )),
        "get_desktop_frame" => get_desktop_frame_response(
            required_str(args, "session")?,
            optional_u32(args, "display")?.map(|value| value as usize),
        ),
        "mouse_move" => tool_payload_result(mouse_move(
            required_str(args, "session")?,
            coordinate(required_u32(args, "x")?, "x")?,
            coordinate(required_u32(args, "y")?, "y")?,
        )),
        "mouse_click" => tool_payload_result(mouse_click(
            required_str(args, "session")?,
            coordinate(required_u32(args, "x")?, "x")?,
            coordinate(required_u32(args, "y")?, "y")?,
            args.get("button")
                .and_then(Value::as_str)
                .unwrap_or("left"),
            args.get("clicks")
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .max(1) as u32,
        )),
        "keyboard_input" => tool_payload_result(keyboard_input(
            required_str(args, "session")?,
            required_str(args, "text")?,
        )),
        "keyboard_hotkey" => {
            let keys = required_string_array(args, "keys")?;
            tool_payload_result(keyboard_hotkey(
                required_str(args, "session")?,
                &keys,
            ))
        }
        _ => {
            return Err(ProtocolError::new(
                -32601,
                format!("unknown tool '{name}'"),
            ))
        }
    })
}

fn tool_payload_result(result: Result<Value, String>) -> Value {
    match result {
        Ok(value) => tool_success(value),
        Err(error) => tool_error(error),
    }
}

fn tool_success(value: Value) -> Value {
    let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value
    })
}

fn tool_success_with_image(
    value: Value,
    summary: impl Into<String>,
    mime_type: &str,
    bytes: &[u8],
) -> Value {
    json!({
        "content": [
            { "type": "text", "text": summary.into() },
            {
                "type": "image",
                "mimeType": mime_type,
                "data": STANDARD.encode(bytes)
            }
        ],
        "structuredContent": value
    })
}

fn tool_error(message: impl Into<String>) -> Value {
    let message = message.into();
    json!({
        "content": [{ "type": "text", "text": message.clone() }],
        "structuredContent": {
            "success": false,
            "error": message.clone()
        },
        "isError": true
    })
}

fn jsonrpc_error(id: Value, code: i32, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message.into()
        }
    })
}

fn open_desktop_session(peer_id: &str) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    clear_local_input_lock_for_peer(peer_id);

    if let Some(snapshot) = latest_desktop_session(peer_id) {
        let session_handle = new_desktop_session_handle(peer_id, Some(snapshot.session_id));
        let connected = snapshot.width > 0 && snapshot.height > 0;
        let needs_password = !connected;
        return Ok(json!({
            "success": true,
            "session": session_handle,
            "actualSession": snapshot.session_id.to_string(),
            "peerId": peer_id,
            "connected": connected,
            "needsPassword": needs_password,
            "display": snapshot.display,
            "displays": displays_json(&snapshot.displays)
        }));
    }

    let before = desktop_session_ids(peer_id);
    crate::run_me(vec!["--connect", peer_id]).map_err(|error| {
        format!("failed to launch the RustDesk desktop connection for '{peer_id}': {error}")
    })?;

    let snapshot = wait_for_desktop_session(peer_id, &before, SESSION_OPEN_TIMEOUT)
        .or_else(|| latest_desktop_session(peer_id))
        .ok_or_else(|| format!("RustDesk did not open a desktop session for '{peer_id}'"))?;
    let session_handle = new_desktop_session_handle(peer_id, Some(snapshot.session_id));

    let connected = snapshot.width > 0 && snapshot.height > 0;
    let needs_password = if connected {
        false
    } else {
        flutter_ffi::session_get_remember(snapshot.session_id)
            .map(|remember| !remember)
            .unwrap_or(true)
    };

    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": snapshot.session_id.to_string(),
        "peerId": peer_id,
        "connected": connected,
        "needsPassword": needs_password,
        "display": snapshot.display,
        "displays": displays_json(&snapshot.displays)
    }))
}

fn input_password(session_handle: &str, password: &str) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_any(session_handle)?;
    let session = any_session(session_handle)?;
    session.queue_password(password.to_string());
    if session.has_login_challenge() {
        flutter_ffi::session_login(
            session_id,
            String::new(),
            String::new(),
            password.to_string(),
            false,
        );
    }
    let connected = wait_for_session_ready(session_id, SESSION_READY_TIMEOUT);
    let snapshot = session_snapshot(session_id);

    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "connected": connected,
        "display": snapshot.as_ref().map(|snapshot| snapshot.display).unwrap_or_default(),
        "displays": snapshot
            .as_ref()
            .map(|snapshot| displays_json(&snapshot.displays))
            .unwrap_or_else(|| json!([]))
    }))
}

fn open_terminal_session(peer_id: &str, rows: u32, cols: u32) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    clear_local_input_lock_for_peer(peer_id);
    let before = terminal_session_ids(peer_id);
    crate::run_me(vec!["--terminal", peer_id]).map_err(|error| {
        format!("failed to launch the RustDesk terminal connection for '{peer_id}': {error}")
    })?;

    let snapshot = wait_for_terminal_session(peer_id, &before, SESSION_OPEN_TIMEOUT)
        .or_else(|| latest_terminal_session(peer_id))
        .ok_or_else(|| format!("RustDesk did not open a terminal session for '{peer_id}'"))?;

    let terminal_id = 1;
    clear_terminal_output(snapshot.session_id, terminal_id);
    flutter_ffi::session_open_terminal(snapshot.session_id, terminal_id, rows.max(10), cols.max(20));
    let session_handle = new_session_handle(peer_id, ConnType::TERMINAL, Some(snapshot.session_id));
    let needs_password = flutter_ffi::session_get_remember(snapshot.session_id)
        .map(|remember| !remember)
        .unwrap_or(true);

    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": snapshot.session_id.to_string(),
        "peerId": peer_id,
        "terminalId": terminal_id,
        "needsPassword": needs_password,
        "rows": rows.max(10),
        "cols": cols.max(20)
    }))
}

fn terminal_input(session_handle: &str, terminal_id: i32, data: &str) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_terminal_session_id(session_handle)?;
    let session = flutter::sessions::get_session_by_session_id(&session_id)
        .ok_or_else(|| format!("unknown terminal session '{session_handle}'"))?;
    let (data, submit) = normalize_terminal_input(session.peer_platform(), data);
    clear_terminal_output(session_id, terminal_id);
    flutter_ffi::session_send_terminal_input(session_id, terminal_id, data.clone());
    if submit {
        let submit_session_id = session_id;
        let submit_terminal_id = terminal_id;
        let _ = std::thread::Builder::new()
            .name("rustdesk-mcp-terminal-submit".to_string())
            .spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                flutter_ffi::session_send_terminal_input(
                    submit_session_id,
                    submit_terminal_id,
                    "\r".to_string(),
                );
            });
    }
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "terminalId": terminal_id,
        "chars": data.chars().count() + if submit { 1 } else { 0 }
    }))
}

fn terminal_output(session_handle: &str, terminal_id: i32, limit: usize) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_terminal_session_id(session_handle)?;
    let raw_output = cached_terminal_output(session_id, terminal_id, limit);
    let output = strip_terminal_ansi(&raw_output);
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "terminalId": terminal_id,
        "output": output,
        "rawOutput": raw_output
    }))
}

fn lock_remote_user_input(session_handle: &str) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let state = desktop_session_registry()
        .lock()
        .unwrap()
        .get(session_handle)
        .cloned()
        .ok_or_else(|| format!("unknown desktop session '{session_handle}'"))?;
    set_local_input_lock_for_peer(&state.peer_id, true);
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "peerId": state.peer_id,
        "locked": true
    }))
}

fn unlock_remote_user_input(session_handle: &str) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let state = desktop_session_registry()
        .lock()
        .unwrap()
        .get(session_handle)
        .cloned()
        .ok_or_else(|| format!("unknown desktop session '{session_handle}'"))?;
    set_local_input_lock_for_peer(&state.peer_id, false);
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "peerId": state.peer_id,
        "locked": false
    }))
}

fn select_desktop_display(session_handle: &str, display: usize) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let snapshot = session_snapshot(session_id)
        .ok_or_else(|| format!("unknown desktop session '{session_handle}'"))?;
    if !snapshot.displays.iter().any(|item| item.index == display) {
        return Err(format!(
            "display {} is not available for desktop session '{}'",
            display, session_handle
        ));
    }
    flutter_ffi::session_switch_display(true, session_id, vec![display as i32]);
    let updated = wait_for_display_switch(session_handle, display, Duration::from_secs(5))
        .unwrap_or(snapshot);
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": updated.session_id.to_string(),
        "display": updated.display,
        "displays": displays_json(&updated.displays)
    }))
}

fn get_desktop_frame_response(
    session_handle: &str,
    requested_display: Option<usize>,
) -> Value {
    let result = get_desktop_frame(session_handle, requested_display);
    match result {
        Ok((payload, png, summary)) => tool_success_with_image(payload, summary, "image/png", &png),
        Err(error) => tool_error(error),
    }
}

fn get_desktop_frame(
    session_handle: &str,
    requested_display: Option<usize>,
) -> Result<(Value, Vec<u8>, String), String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let _session = desktop_session(session_handle)?;
    let snapshot = session_snapshot(session_id)
        .ok_or_else(|| format!("unknown desktop session '{session_handle}'"))?;
    let display = requested_display.unwrap_or(snapshot.display);
    if !snapshot.displays.iter().any(|item| item.index == display) {
        return Err(format!(
            "display {display} is not available for desktop session '{session_id}'"
        ));
    }
    let frame = wait_for_frame(session_id, display, FRAME_WAIT_TIMEOUT).ok_or_else(|| {
        format!("no desktop frame is ready yet for session '{session_id}' display {display}")
    })?;
    let png = encode_png(frame.width, frame.height, &frame.rgba)?;
    let payload = json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "display": frame.display,
        "width": frame.width,
        "height": frame.height,
        "displays": displays_json(&snapshot.displays)
    });
    let summary = format!(
        "desktop frame for session {} display {} ({}x{})",
        session_handle, frame.display, frame.width, frame.height
    );
    Ok((payload, png, summary))
}

fn mouse_move(session_handle: &str, x: i32, y: i32) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let session = desktop_session(session_handle)?;
    session.send_mouse_agent(MOUSE_TYPE_MOVE, x, y, false, false, false, false);
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "x": x,
        "y": y
    }))
}

fn mouse_click(
    session_handle: &str,
    x: i32,
    y: i32,
    button: &str,
    clicks: u32,
) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let session = desktop_session(session_handle)?;
    let button_mask = mouse_button_mask(button)?;
    session.send_mouse_agent(MOUSE_TYPE_MOVE, x, y, false, false, false, false);
    for _ in 0..clicks.max(1) {
        session.send_mouse_agent(
            button_mask << 3 | MOUSE_TYPE_DOWN,
            x,
            y,
            false,
            false,
            false,
            false,
        );
        session.send_mouse_agent(
            button_mask << 3 | MOUSE_TYPE_UP,
            x,
            y,
            false,
            false,
            false,
            false,
        );
    }
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "x": x,
        "y": y,
        "button": button,
        "clicks": clicks.max(1)
    }))
}

fn keyboard_input(session_handle: &str, text: &str) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let session = desktop_session(session_handle)?;
    let mut key_event = KeyEvent::new();
    key_event.mode = KeyboardMode::Legacy.into();
    key_event.press = true;
    key_event.set_seq(text.to_string());
    session.send_key_event_agent(&key_event);
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "chars": text.chars().count()
    }))
}

fn keyboard_hotkey(session_handle: &str, keys: &[String]) -> Result<Value, String> {
    ensure_mcp_enabled()?;
    let session_id = resolve_actual_session_id(session_handle)?;
    let session = desktop_session(session_handle)?;
    let hotkey = Hotkey::parse(keys)?;
    with_agent_input_bypass(|| -> Result<(), String> {
        let keyboard_mode = session.get_keyboard_mode();
        for modifier in &hotkey.modifiers {
            let key = modifier_rdev_key(*modifier)
                .ok_or_else(|| format!("unsupported modifier '{modifier:?}'"))?;
            let usb_hid = rdev::usb_hid_keycode_from_key(key)
                .ok_or_else(|| format!("missing usb hid mapping for modifier '{modifier:?}'"))?
                as i32;
            session.handle_flutter_key_event(&keyboard_mode, "", usb_hid, 0, true);
        }

        if let Some(target) = hotkey.target {
            match target {
                HotkeyTarget::Control(control_key) => {
                    let key = control_rdev_key(control_key)
                        .ok_or_else(|| format!("unsupported control key '{control_key:?}'"))?;
                    let usb_hid = rdev::usb_hid_keycode_from_key(key)
                        .ok_or_else(|| {
                            format!("missing usb hid mapping for control key '{control_key:?}'")
                        })? as i32;
                    session.handle_flutter_key_event(&keyboard_mode, "", usb_hid, 0, true);
                    session.handle_flutter_key_event(&keyboard_mode, "", usb_hid, 0, false);
                }
                HotkeyTarget::Seq(seq) => {
                    let ch = seq
                        .chars()
                        .next()
                        .ok_or_else(|| "empty hotkey sequence".to_string())?;
                    let key = char_rdev_key(ch)
                        .ok_or_else(|| format!("unsupported hotkey key '{seq}'"))?;
                    let usb_hid = rdev::usb_hid_keycode_from_key(key)
                        .ok_or_else(|| format!("missing usb hid mapping for key '{seq}'"))?
                        as i32;
                    session.handle_flutter_key_event(
                        &keyboard_mode,
                        &ch.to_string(),
                        usb_hid,
                        0,
                        true,
                    );
                    session.handle_flutter_key_event(
                        &keyboard_mode,
                        &ch.to_string(),
                        usb_hid,
                        0,
                        false,
                    );
                }
            }
        }

        for modifier in hotkey.modifiers.iter().rev() {
            let key = modifier_rdev_key(*modifier)
                .ok_or_else(|| format!("unsupported modifier '{modifier:?}'"))?;
            let usb_hid = rdev::usb_hid_keycode_from_key(key)
                .ok_or_else(|| format!("missing usb hid mapping for modifier '{modifier:?}'"))?
                as i32;
            session.handle_flutter_key_event(&keyboard_mode, "", usb_hid, 0, false);
        }
        Ok(())
    })?;
    Ok(json!({
        "success": true,
        "session": session_handle,
        "actualSession": session_id.to_string(),
        "keys": keys
    }))
}

fn mcp_enabled() -> bool {
    let value = LocalConfig::get_option(MCP_ENABLE_OPTION);
    if value.is_empty() {
        LocalConfig::get_option(MCP_ENABLE_OPTION_LEGACY) == "Y"
    } else {
        value == "Y"
    }
}

fn ensure_mcp_enabled() -> Result<(), String> {
    if mcp_enabled() {
        Ok(())
    } else {
        Err("MCP server is disabled. Enable it in Settings -> Security -> Enable MCP server."
            .to_string())
    }
}

fn desktop_session(session_handle: &str) -> Result<FlutterSession, String> {
    let session_id = resolve_actual_session_id(session_handle)?;
    let session = flutter::sessions::get_session_by_session_id(&session_id)
        .ok_or_else(|| format!("unknown desktop session '{session_handle}'"))?;
    let conn_type = session.lc.read().unwrap().conn_type;
    if conn_type != ConnType::DEFAULT_CONN {
        return Err(format!("session '{session_handle}' is not a desktop session"));
    }
    Ok(session)
}

fn desktop_session_ids(peer_id: &str) -> HashSet<SessionID> {
    desktop_snapshots(peer_id)
        .into_iter()
        .map(|snapshot| snapshot.session_id)
        .collect()
}

fn latest_desktop_session(peer_id: &str) -> Option<AgentBridgeSessionSnapshot> {
    let snapshots = desktop_snapshots(peer_id);
    if snapshots.is_empty() {
        return None;
    }
    snapshots
        .into_iter()
        .max_by_key(|snapshot| {
            cached_frame_timestamp(snapshot.session_id, snapshot.display).unwrap_or(0)
        })
}

fn terminal_session_ids(peer_id: &str) -> HashSet<SessionID> {
    terminal_snapshots(peer_id)
        .into_iter()
        .map(|snapshot| snapshot.session_id)
        .collect()
}

fn latest_terminal_session(peer_id: &str) -> Option<AgentBridgeSessionSnapshot> {
    terminal_snapshots(peer_id)
        .into_iter()
        .max_by_key(|snapshot| snapshot.session_id)
}

fn new_desktop_session_handle(peer_id: &str, actual_session: Option<SessionID>) -> String {
    new_session_handle(peer_id, ConnType::DEFAULT_CONN, actual_session)
}

fn new_session_handle(
    peer_id: &str,
    conn_type: ConnType,
    actual_session: Option<SessionID>,
) -> String {
    let handle = SessionID::new_v4().to_string();
    desktop_session_registry().lock().unwrap().insert(
        handle.clone(),
        DesktopSessionHandle {
            peer_id: peer_id.to_string(),
            conn_type,
            last_actual_session: actual_session,
        },
    );
    handle
}

fn resolve_actual_session_id(session_handle: &str) -> Result<SessionID, String> {
    if let Ok(session_id) = SessionID::from_str(session_handle) {
        if let Some(session) = flutter::sessions::get_session_by_session_id(&session_id) {
            if session.lc.read().unwrap().conn_type == ConnType::DEFAULT_CONN {
                return Ok(session_id);
            }
        }
    }

    let state = desktop_session_registry()
        .lock()
        .unwrap()
        .get(session_handle)
        .cloned()
        .ok_or_else(|| format!("unknown desktop session '{session_handle}'"))?;

    if flutter::get_cur_peer_id() == state.peer_id {
        let current_session_id = flutter::get_cur_session_id();
        if let Some(session) = flutter::sessions::get_session_by_session_id(&current_session_id) {
            if session.lc.read().unwrap().conn_type == ConnType::DEFAULT_CONN {
                desktop_session_registry()
                    .lock()
                    .unwrap()
                    .entry(session_handle.to_string())
                    .and_modify(|entry| entry.last_actual_session = Some(current_session_id));
                return Ok(current_session_id);
            }
        }
    }

    if let Some(snapshot) = latest_desktop_session(&state.peer_id) {
        desktop_session_registry()
            .lock()
            .unwrap()
            .entry(session_handle.to_string())
            .and_modify(|entry| entry.last_actual_session = Some(snapshot.session_id));
        return Ok(snapshot.session_id);
    }

    if let Some(session_id) = state.last_actual_session {
        if let Some(session) = flutter::sessions::get_session_by_session_id(&session_id) {
            if session.lc.read().unwrap().conn_type == ConnType::DEFAULT_CONN {
                return Ok(session_id);
            }
        }
    }

    let snapshot = wait_for_any_desktop_session(&state.peer_id, Duration::from_secs(5))
        .ok_or_else(|| {
            format!(
                "desktop session '{}' for peer '{}' is not ready",
                session_handle, state.peer_id
            )
        })?;
    desktop_session_registry()
        .lock()
        .unwrap()
        .entry(session_handle.to_string())
        .and_modify(|entry| entry.last_actual_session = Some(snapshot.session_id));
    Ok(snapshot.session_id)
}

fn resolve_actual_terminal_session_id(session_handle: &str) -> Result<SessionID, String> {
    let state = desktop_session_registry()
        .lock()
        .unwrap()
        .get(session_handle)
        .cloned()
        .ok_or_else(|| format!("unknown terminal session '{session_handle}'"))?;

    if let Some(snapshot) = latest_terminal_session(&state.peer_id) {
        desktop_session_registry()
            .lock()
            .unwrap()
            .entry(session_handle.to_string())
            .and_modify(|entry| entry.last_actual_session = Some(snapshot.session_id));
        return Ok(snapshot.session_id);
    }

    if let Some(session_id) = state.last_actual_session {
        if let Some(session) = flutter::sessions::get_session_by_session_id(&session_id) {
            if session.lc.read().unwrap().conn_type == ConnType::TERMINAL {
                return Ok(session_id);
            }
        }
    }

    Err(format!(
        "terminal session '{}' for peer '{}' is not ready",
        session_handle, state.peer_id
    ))
}

fn resolve_actual_session_any(session_handle: &str) -> Result<SessionID, String> {
    let state = desktop_session_registry()
        .lock()
        .unwrap()
        .get(session_handle)
        .cloned()
        .ok_or_else(|| format!("unknown session '{session_handle}'"))?;
    match state.conn_type {
        ConnType::DEFAULT_CONN => resolve_actual_session_id(session_handle),
        ConnType::TERMINAL => resolve_actual_terminal_session_id(session_handle),
        _ => Err(format!("unsupported MCP session type for '{session_handle}'")),
    }
}

fn any_session(session_handle: &str) -> Result<FlutterSession, String> {
    let session_id = resolve_actual_session_any(session_handle)?;
    flutter::sessions::get_session_by_session_id(&session_id)
        .ok_or_else(|| format!("unknown session '{session_handle}'"))
}

fn session_snapshot(session_id: SessionID) -> Option<AgentBridgeSessionSnapshot> {
    flutter::agent_bridge_list_sessions()
        .into_iter()
        .find(|snapshot| snapshot.session_id == session_id)
}

fn desktop_snapshots(peer_id: &str) -> Vec<AgentBridgeSessionSnapshot> {
    flutter::agent_bridge_list_sessions()
        .into_iter()
        .filter(|snapshot| {
            snapshot.peer_id == peer_id && snapshot.conn_type == ConnType::DEFAULT_CONN
        })
        .collect()
}

fn terminal_snapshots(peer_id: &str) -> Vec<AgentBridgeSessionSnapshot> {
    flutter::agent_bridge_list_sessions()
        .into_iter()
        .filter(|snapshot| snapshot.peer_id == peer_id && snapshot.conn_type == ConnType::TERMINAL)
        .collect()
}

fn wait_for_desktop_session(
    peer_id: &str,
    before: &HashSet<SessionID>,
    timeout: Duration,
) -> Option<AgentBridgeSessionSnapshot> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(snapshot) = desktop_snapshots(peer_id)
            .into_iter()
            .find(|snapshot| !before.contains(&snapshot.session_id))
        {
            return Some(snapshot);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

fn wait_for_terminal_session(
    peer_id: &str,
    before: &HashSet<SessionID>,
    timeout: Duration,
) -> Option<AgentBridgeSessionSnapshot> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(snapshot) = terminal_snapshots(peer_id)
            .into_iter()
            .find(|snapshot| !before.contains(&snapshot.session_id))
        {
            return Some(snapshot);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

fn wait_for_session_ready(session_id: SessionID, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if session_snapshot(session_id)
            .map(|snapshot| snapshot.width > 0 && snapshot.height > 0)
            .unwrap_or(false)
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    false
}

fn wait_for_any_desktop_session(
    peer_id: &str,
    timeout: Duration,
) -> Option<AgentBridgeSessionSnapshot> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(snapshot) = latest_desktop_session(peer_id) {
            return Some(snapshot);
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    None
}

fn wait_for_display_switch(
    session_handle: &str,
    display: usize,
    timeout: Duration,
) -> Option<AgentBridgeSessionSnapshot> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        let session_id = resolve_actual_session_id(session_handle).ok()?;
        if let Some(snapshot) = session_snapshot(session_id) {
            if snapshot.display == display {
                return Some(snapshot);
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    None
}

fn wait_for_frame(
    session_id: SessionID,
    display: usize,
    timeout: Duration,
) -> Option<AgentBridgeFrameSnapshot> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(frame) = cached_frame(session_id, display) {
            return Some(frame);
        }
        if let Some(frame) = flutter::agent_bridge_get_frame(&session_id, display) {
            return Some(frame);
        }
        std::thread::sleep(Duration::from_millis(150));
    }
    None
}

fn displays_json(displays: &[AgentBridgeDisplaySnapshot]) -> Value {
    Value::Array(
        displays
            .iter()
            .map(|display| {
                json!({
                    "index": display.index,
                    "width": display.width,
                    "height": display.height
                })
            })
            .collect(),
    )
}

fn encode_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    let mut encoder = repng::Options::smallest(width, height)
        .build(&mut png)
        .map_err(|error| format!("failed to create PNG encoder: {error}"))?;
    encoder
        .write(rgba)
        .map_err(|error| format!("failed to encode desktop frame: {error}"))?;
    encoder
        .finish()
        .map_err(|error| format!("failed to finalize desktop frame PNG: {error}"))?;
    Ok(png)
}

fn coordinate(value: u32, name: &str) -> Result<i32, ProtocolError> {
    value.try_into().map_err(|_| {
        ProtocolError::new(
            -32602,
            format!("{name} is outside RustDesk's pointer coordinate range"),
        )
    })
}

fn required_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, ProtocolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ProtocolError::new(-32602, format!("missing or invalid string '{key}'")))
}

fn required_u32(args: &Map<String, Value>, key: &str) -> Result<u32, ProtocolError> {
    let value = args
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| ProtocolError::new(-32602, format!("missing or invalid integer '{key}'")))?;
    value.try_into().map_err(|_| {
        ProtocolError::new(
            -32602,
            format!("'{key}' is outside the supported integer range"),
        )
    })
}

fn optional_u32(args: &Map<String, Value>, key: &str) -> Result<Option<u32>, ProtocolError> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let value = value
        .as_u64()
        .ok_or_else(|| ProtocolError::new(-32602, format!("'{key}' must be an integer")))?;
    let value = value.try_into().map_err(|_| {
        ProtocolError::new(
            -32602,
            format!("'{key}' is outside the supported integer range"),
        )
    })?;
    Ok(Some(value))
}

fn required_i32(args: &Map<String, Value>, key: &str) -> Result<i32, ProtocolError> {
    let value = args
        .get(key)
        .and_then(Value::as_i64)
        .ok_or_else(|| ProtocolError::new(-32602, format!("missing or invalid integer '{key}'")))?;
    value.try_into().map_err(|_| {
        ProtocolError::new(
            -32602,
            format!("'{key}' is outside the supported integer range"),
        )
    })
}

fn required_string_array(
    args: &Map<String, Value>,
    key: &str,
) -> Result<Vec<String>, ProtocolError> {
    let items = args
        .get(key)
        .and_then(Value::as_array)
        .ok_or_else(|| ProtocolError::new(-32602, format!("missing or invalid array '{key}'")))?;
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        let value = item
            .as_str()
            .ok_or_else(|| ProtocolError::new(-32602, format!("'{key}' must contain strings")))?;
        values.push(value.to_string());
    }
    if values.is_empty() {
        return Err(ProtocolError::new(
            -32602,
            format!("'{key}' must not be empty"),
        ));
    }
    Ok(values)
}

#[derive(Debug, Default)]
struct Hotkey {
    modifiers: Vec<ControlKey>,
    target: Option<HotkeyTarget>,
}

#[derive(Debug, Clone)]
enum HotkeyTarget {
    Control(ControlKey),
    Seq(String),
}

impl Hotkey {
    fn parse(keys: &[String]) -> Result<Self, String> {
        if keys.is_empty() {
            return Err("hotkey must not be empty".to_string());
        }

        let mut hotkey = Hotkey::default();
        for key in keys {
            let normalized = normalize_key(key);
            if let Some(modifier) = modifier_key(&normalized) {
                hotkey.modifiers.push(modifier);
                continue;
            }
            hotkey.target = Some(match control_key(&normalized) {
                Some(control_key) => HotkeyTarget::Control(control_key),
                None => {
                    if normalized.chars().count() == 1 {
                        HotkeyTarget::Seq(normalized)
                    } else {
                        return Err(format!("unsupported hotkey key '{key}'"));
                    }
                }
            });
        }
        Ok(hotkey)
    }
}

fn mouse_button_mask(button: &str) -> Result<i32, String> {
    match button.trim().to_ascii_lowercase().as_str() {
        "" | "left" => Ok(MOUSE_BUTTON_LEFT),
        "right" => Ok(MOUSE_BUTTON_RIGHT),
        "middle" | "wheel" => Ok(MOUSE_BUTTON_WHEEL),
        value => Err(format!("unsupported mouse button '{value}'")),
    }
}

fn normalize_key(key: &str) -> String {
    key.trim()
        .to_ascii_lowercase()
        .replace('-', "")
        .replace('_', "")
        .replace(' ', "")
}

fn modifier_key(key: &str) -> Option<ControlKey> {
    match key {
        "ctrl" | "control" => Some(ControlKey::Control),
        "alt" | "option" => Some(ControlKey::Alt),
        "shift" => Some(ControlKey::Shift),
        "meta" | "cmd" | "command" | "win" | "windows" | "super" => Some(ControlKey::Meta),
        _ => None,
    }
}

fn control_key(key: &str) -> Option<ControlKey> {
    match key {
        "backspace" => Some(ControlKey::Backspace),
        "capslock" => Some(ControlKey::CapsLock),
        "delete" | "del" => Some(ControlKey::Delete),
        "down" | "downarrow" => Some(ControlKey::DownArrow),
        "end" => Some(ControlKey::End),
        "esc" | "escape" => Some(ControlKey::Escape),
        "home" => Some(ControlKey::Home),
        "insert" | "ins" => Some(ControlKey::Insert),
        "left" | "leftarrow" => Some(ControlKey::LeftArrow),
        "pagedown" => Some(ControlKey::PageDown),
        "pageup" => Some(ControlKey::PageUp),
        "enter" | "return" => Some(ControlKey::Return),
        "right" | "rightarrow" => Some(ControlKey::RightArrow),
        "space" => Some(ControlKey::Space),
        "tab" => Some(ControlKey::Tab),
        "up" | "uparrow" => Some(ControlKey::UpArrow),
        "ctrlaltdel" | "ctrlaltdelete" => Some(ControlKey::CtrlAltDel),
        "lockscreen" => Some(ControlKey::LockScreen),
        "f1" => Some(ControlKey::F1),
        "f2" => Some(ControlKey::F2),
        "f3" => Some(ControlKey::F3),
        "f4" => Some(ControlKey::F4),
        "f5" => Some(ControlKey::F5),
        "f6" => Some(ControlKey::F6),
        "f7" => Some(ControlKey::F7),
        "f8" => Some(ControlKey::F8),
        "f9" => Some(ControlKey::F9),
        "f10" => Some(ControlKey::F10),
        "f11" => Some(ControlKey::F11),
        "f12" => Some(ControlKey::F12),
        _ => None,
    }
}

fn modifier_rdev_key(key: ControlKey) -> Option<rdev::Key> {
    match key {
        ControlKey::Control => Some(rdev::Key::ControlLeft),
        ControlKey::Alt => Some(rdev::Key::Alt),
        ControlKey::Shift => Some(rdev::Key::ShiftLeft),
        ControlKey::Meta => Some(rdev::Key::MetaLeft),
        _ => None,
    }
}

fn control_rdev_key(key: ControlKey) -> Option<rdev::Key> {
    match key {
        ControlKey::Backspace => Some(rdev::Key::Backspace),
        ControlKey::CapsLock => Some(rdev::Key::CapsLock),
        ControlKey::Delete => Some(rdev::Key::Delete),
        ControlKey::DownArrow => Some(rdev::Key::DownArrow),
        ControlKey::End => Some(rdev::Key::End),
        ControlKey::Escape => Some(rdev::Key::Escape),
        ControlKey::Home => Some(rdev::Key::Home),
        ControlKey::Insert => Some(rdev::Key::Insert),
        ControlKey::LeftArrow => Some(rdev::Key::LeftArrow),
        ControlKey::PageDown => Some(rdev::Key::PageDown),
        ControlKey::PageUp => Some(rdev::Key::PageUp),
        ControlKey::Return => Some(rdev::Key::Return),
        ControlKey::RightArrow => Some(rdev::Key::RightArrow),
        ControlKey::Space => Some(rdev::Key::Space),
        ControlKey::Tab => Some(rdev::Key::Tab),
        ControlKey::UpArrow => Some(rdev::Key::UpArrow),
        ControlKey::F1 => Some(rdev::Key::F1),
        ControlKey::F2 => Some(rdev::Key::F2),
        ControlKey::F3 => Some(rdev::Key::F3),
        ControlKey::F4 => Some(rdev::Key::F4),
        ControlKey::F5 => Some(rdev::Key::F5),
        ControlKey::F6 => Some(rdev::Key::F6),
        ControlKey::F7 => Some(rdev::Key::F7),
        ControlKey::F8 => Some(rdev::Key::F8),
        ControlKey::F9 => Some(rdev::Key::F9),
        ControlKey::F10 => Some(rdev::Key::F10),
        ControlKey::F11 => Some(rdev::Key::F11),
        ControlKey::F12 => Some(rdev::Key::F12),
        _ => None,
    }
}

fn char_rdev_key(ch: char) -> Option<rdev::Key> {
    match ch.to_ascii_lowercase() {
        'a' => Some(rdev::Key::KeyA),
        'b' => Some(rdev::Key::KeyB),
        'c' => Some(rdev::Key::KeyC),
        'd' => Some(rdev::Key::KeyD),
        'e' => Some(rdev::Key::KeyE),
        'f' => Some(rdev::Key::KeyF),
        'g' => Some(rdev::Key::KeyG),
        'h' => Some(rdev::Key::KeyH),
        'i' => Some(rdev::Key::KeyI),
        'j' => Some(rdev::Key::KeyJ),
        'k' => Some(rdev::Key::KeyK),
        'l' => Some(rdev::Key::KeyL),
        'm' => Some(rdev::Key::KeyM),
        'n' => Some(rdev::Key::KeyN),
        'o' => Some(rdev::Key::KeyO),
        'p' => Some(rdev::Key::KeyP),
        'q' => Some(rdev::Key::KeyQ),
        'r' => Some(rdev::Key::KeyR),
        's' => Some(rdev::Key::KeyS),
        't' => Some(rdev::Key::KeyT),
        'u' => Some(rdev::Key::KeyU),
        'v' => Some(rdev::Key::KeyV),
        'w' => Some(rdev::Key::KeyW),
        'x' => Some(rdev::Key::KeyX),
        'y' => Some(rdev::Key::KeyY),
        'z' => Some(rdev::Key::KeyZ),
        '0' => Some(rdev::Key::Num0),
        '1' => Some(rdev::Key::Num1),
        '2' => Some(rdev::Key::Num2),
        '3' => Some(rdev::Key::Num3),
        '4' => Some(rdev::Key::Num4),
        '5' => Some(rdev::Key::Num5),
        '6' => Some(rdev::Key::Num6),
        '7' => Some(rdev::Key::Num7),
        '8' => Some(rdev::Key::Num8),
        '9' => Some(rdev::Key::Num9),
        _ => None,
    }
}

fn read_http_request(stream: &TcpStream) -> io::Result<Option<HttpRequest>> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(None);
    }
    let request_line = request_line.trim_end_matches(['\r', '\n']);
    if request_line.is_empty() {
        return Ok(None);
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let path = parts.next().unwrap_or_default().to_string();
    let _version = parts.next().unwrap_or_default().to_string();

    let mut headers = HashMap::new();
    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            break;
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            let key = name.trim().to_ascii_lowercase();
            let value = value.trim().to_string();
            if key == "content-length" {
                content_length = value.parse::<usize>().unwrap_or_default();
            }
            headers.insert(key, value);
        }
    }

    let mut body = vec![0; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(Some(HttpRequest {
        method,
        path,
        headers,
        body,
    }))
}

fn write_http_response(stream: &mut TcpStream, response: HttpResponse) -> io::Result<()> {
    let mut headers = vec![
        ("Connection".to_string(), "close".to_string()),
        ("Content-Length".to_string(), response.body.len().to_string()),
        (
            "Access-Control-Allow-Origin".to_string(),
            "*".to_string(),
        ),
        (
            "Access-Control-Allow-Headers".to_string(),
            "Content-Type, Accept, MCP-Protocol-Version".to_string(),
        ),
        (
            "Access-Control-Allow-Methods".to_string(),
            "POST, GET, OPTIONS".to_string(),
        ),
        (
            "MCP-Protocol-Version".to_string(),
            MCP_PROTOCOL_VERSION.to_string(),
        ),
    ];
    if let Some(content_type) = response.content_type {
        headers.push(("Content-Type".to_string(), content_type.to_string()));
    }
    headers.extend(response.headers);

    let mut out = format!("HTTP/1.1 {} {}\r\n", response.status_code, response.reason);
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");

    stream.write_all(out.as_bytes())?;
    if !response.body.is_empty() {
        stream.write_all(&response.body)?;
    }
    stream.flush()
}

struct HttpRequest {
    method: String,
    path: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

struct HttpResponse {
    status_code: u16,
    reason: &'static str,
    content_type: Option<&'static str>,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn empty(status_code: u16, reason: &'static str) -> Self {
        Self {
            status_code,
            reason,
            content_type: None,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn text(status_code: u16, reason: &'static str, body: impl Into<String>) -> Self {
        Self {
            status_code,
            reason,
            content_type: Some("text/plain; charset=utf-8"),
            headers: Vec::new(),
            body: body.into().into_bytes(),
        }
    }

    fn json(status_code: u16, reason: &'static str, body: Value) -> Self {
        Self {
            status_code,
            reason,
            content_type: Some("application/json; charset=utf-8"),
            headers: Vec::new(),
            body: body.to_string().into_bytes(),
        }
    }
}

#[derive(Debug, Clone)]
struct DesktopSessionHandle {
    peer_id: String,
    conn_type: ConnType,
    last_actual_session: Option<SessionID>,
}

fn desktop_session_registry() -> &'static Mutex<HashMap<String, DesktopSessionHandle>> {
    DESKTOP_SESSION_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn should_drop_user_input(peer_id: &str) -> bool {
    let bypassed = AGENT_INPUT_BYPASS.with(|flag| flag.get() > 0);
    if bypassed {
        return false;
    }
    LOCKED_REMOTE_INPUT_PEERS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .map(|locked| locked.contains(peer_id))
        .unwrap_or(false)
}

fn set_local_input_lock_for_peer(peer_id: &str, locked: bool) {
    let peers = LOCKED_REMOTE_INPUT_PEERS.get_or_init(|| Mutex::new(HashSet::new()));
    if let Ok(mut peers) = peers.lock() {
        if locked {
            peers.insert(peer_id.to_string());
        } else {
            peers.remove(peer_id);
        }
    }
}

pub fn clear_local_input_lock_for_peer(peer_id: &str) {
    set_local_input_lock_for_peer(peer_id, false);
}

pub fn record_terminal_response(session_ids: &[SessionID], response: &TerminalResponse) {
    let Some((terminal_id, text)) = terminal_response_text(response) else {
        return;
    };
    let cache = TERMINAL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return;
    };
    for session_id in session_ids {
        let entry = cache.entry((*session_id, terminal_id)).or_default();
        entry.push_str(&text);
        if entry.len() > 262_144 {
            let keep_from = entry.len() - 262_144;
            *entry = entry[keep_from..].to_string();
        }
    }
}

fn terminal_response_text(response: &TerminalResponse) -> Option<(i32, String)> {
    match response.union.as_ref()? {
        terminal_response::Union::Opened(opened) => Some((
            opened.terminal_id,
            format!(
                "[terminal opened {} success={}]\n",
                opened.terminal_id, opened.success
            ),
        )),
        terminal_response::Union::Data(data) => {
            let output_data = if data.compressed {
                hbb_common::compress::decompress(&data.data)
            } else {
                data.data.to_vec()
            };
            Some((data.terminal_id, decode_terminal_bytes(&output_data)))
        }
        terminal_response::Union::Closed(closed) => Some((
            closed.terminal_id,
            format!(
                "\n[terminal closed {}, exit_code={}]\n",
                closed.terminal_id, closed.exit_code
            ),
        )),
        terminal_response::Union::Error(error) => Some((
            error.terminal_id,
            format!("\n[terminal error {}: {}]\n", error.terminal_id, error.message),
        )),
        _ => None,
    }
}

fn cached_terminal_output(session_id: SessionID, terminal_id: i32, limit: usize) -> String {
    let output = TERMINAL_CACHE
        .get()
        .and_then(|cache| cache.lock().ok())
        .and_then(|cache| cache.get(&(session_id, terminal_id)).cloned())
        .unwrap_or_default();
    let char_len = output.chars().count();
    if char_len <= limit {
        output
    } else {
        output
            .chars()
            .skip(char_len.saturating_sub(limit))
            .collect()
    }
}

fn clear_terminal_output(session_id: SessionID, terminal_id: i32) {
    let cache = TERMINAL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(mut cache) = cache.lock() {
        cache.remove(&(session_id, terminal_id));
    }
}

fn decode_terminal_bytes(bytes: &[u8]) -> String {
    let decoded = match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => {
            let (decoded, _, had_errors) = GBK.decode(bytes);
            if had_errors {
                String::from_utf8_lossy(bytes).to_string()
            } else {
                decoded.into_owned()
            }
        }
    };
    repair_utf8_mojibake(&decoded)
}

fn normalize_terminal_input(platform: String, data: &str) -> (String, bool) {
    let _is_windows = platform.eq_ignore_ascii_case("Windows");
    if let Some(stripped) = data.strip_suffix("\r\n") {
        return (stripped.to_string(), true);
    }
    if let Some(stripped) = data.strip_suffix('\n') {
        return (stripped.to_string(), true);
    }
    (data.to_string(), false)
}

fn repair_utf8_mojibake(input: &str) -> String {
    if !input.chars().any(|ch| matches!(ch as u32, 0x80..=0x9f | 0x00c0..=0x00ff)) {
        return input.to_string();
    }

    let mut bytes = Vec::with_capacity(input.len());
    for ch in input.chars() {
        let code = ch as u32;
        if code <= 0x00ff {
            bytes.push(code as u8);
        } else {
            return input.to_string();
        }
    }

    let Ok(repaired) = String::from_utf8(bytes) else {
        return input.to_string();
    };

    let repaired_non_ascii = repaired.chars().filter(|ch| !ch.is_ascii()).count();
    let input_non_ascii = input.chars().filter(|ch| !ch.is_ascii()).count();
    let has_c1_controls = input.chars().any(|ch| matches!(ch as u32, 0x80..=0x9f));
    if repaired_non_ascii > input_non_ascii || has_c1_controls {
        repaired
    } else {
        input.to_string()
    }
}

fn strip_terminal_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    let mut copy_start = 0usize;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if copy_start < i {
                if let Ok(text) = std::str::from_utf8(&bytes[copy_start..i]) {
                    out.push_str(text);
                }
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                i += 2;
                while i < bytes.len() {
                    let b = bytes[i];
                    if (0x40..=0x7e).contains(&b) {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                copy_start = i;
                continue;
            }
            if i + 1 < bytes.len() && bytes[i + 1] == b']' {
                i += 2;
                while i < bytes.len() {
                    if bytes[i] == 0x07 {
                        i += 1;
                        break;
                    }
                    if i + 1 < bytes.len() && bytes[i] == 0x1b && bytes[i + 1] == b'\\' {
                        i += 2;
                        break;
                    }
                    i += 1;
                }
                copy_start = i;
                continue;
            }
        }
        i += 1;
    }
    if copy_start < bytes.len() {
        if let Ok(text) = std::str::from_utf8(&bytes[copy_start..]) {
            out.push_str(text);
        }
    }
    out.replace('\r', "")
}

fn with_agent_input_bypass<T>(f: impl FnOnce() -> T) -> T {
    AGENT_INPUT_BYPASS.with(|flag| {
        let current = flag.get();
        flag.set(current + 1);
        let result = f();
        flag.set(current);
        result
    })
}

#[derive(Debug, Clone)]
struct CachedFrame {
    session_id: SessionID,
    display: usize,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    captured_at_ms: u64,
}

pub fn record_frame(session_ids: &[SessionID], display: usize, rgba: &scrap::ImageRgb) {
    if session_ids.is_empty() || rgba.raw.is_empty() || rgba.w == 0 || rgba.h == 0 {
        return;
    }
    let width = rgba.w.min(u32::MAX as usize) as u32;
    let height = rgba.h.min(u32::MAX as usize) as u32;
    let captured_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or_default();
    let cache = FRAME_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let Ok(mut cache) = cache.lock() else {
        return;
    };
    for session_id in session_ids {
        cache.insert(
            (*session_id, display),
            CachedFrame {
                session_id: *session_id,
                display,
                width,
                height,
                rgba: rgba.raw.clone(),
                captured_at_ms,
            },
        );
    }
}

fn cached_frame(session_id: SessionID, display: usize) -> Option<AgentBridgeFrameSnapshot> {
    let cache = FRAME_CACHE.get()?;
    let cache = cache.lock().ok()?;
    let frame = cache.get(&(session_id, display))?.clone();
    Some(AgentBridgeFrameSnapshot {
        session_id: frame.session_id,
        display: frame.display,
        width: frame.width,
        height: frame.height,
        rgba: frame.rgba,
    })
}

fn cached_frame_timestamp(session_id: SessionID, display: usize) -> Option<u64> {
    let cache = FRAME_CACHE.get()?;
    let cache = cache.lock().ok()?;
    cache.get(&(session_id, display)).map(|frame| frame.captured_at_ms)
}

#[derive(Debug)]
struct ProtocolError {
    code: i32,
    message: String,
}

impl ProtocolError {
    fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}
