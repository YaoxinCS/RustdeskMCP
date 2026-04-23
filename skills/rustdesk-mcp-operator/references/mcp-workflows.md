# RustDeskMCP workflows

## Scope

- Runtime package:
  - EXE: `test-runtime/RustDeskMCP/RustDeskMCP.exe`
  - DLL: `test-runtime/RustDeskMCP/librustdesk.dll`
- Local MCP endpoint: `http://127.0.0.1:59940/mcp`
- Normal GUI startup is the supported path. Do not depend on `--mcp`.
- GUI setting path: `Settings -> Security -> Enable MCP server`
- Separate app identity is intentional:
  - `RustDeskMCP`
  - env key: `RUSTDESK_MCP_APP_NAME`

## Build and refresh loop

- Validation commands:
  - `cargo check --features flutter`
  - `cargo build --release --features flutter`
- Set any required local build environment variables according to your own machine and dependency installation.
- After a successful release build, refresh the runtime DLL:
  - source: `target/release/librustdesk.dll`
  - target: `test-runtime/RustDeskMCP/librustdesk.dll`
- Restart `RustDeskMCP.exe` after replacing the DLL.

## MCP transport

- Use JSON-RPC 2.0 over `POST /mcp`.
- Normal sequence:
  1. `initialize`
  2. `tools/list`
  3. `tools/call`
- The built-in server exposes no resources or prompts beyond empty lists.

### PowerShell helper

```powershell
function Invoke-McpTool($id, $name, $arguments) {
  $body = @{
    jsonrpc = '2.0'
    id = $id
    method = 'tools/call'
    params = @{
      name = $name
      arguments = $arguments
    }
  } | ConvertTo-Json -Depth 10

  Invoke-RestMethod `
    -Uri 'http://127.0.0.1:59940/mcp' `
    -Method Post `
    -ContentType 'application/json' `
    -Body $body
}
```

## Implemented tools

### Desktop session tools

- `open_desktop_session(id)`
  - Opens a desktop session through the normal RustDesk user path.
  - Returns `session`, `actualSession`, `peerId`, `connected`, `needsPassword`, `display`, `displays`.
- `input_password(session, password)`
  - Works for both desktop and terminal handles.
  - For desktop, `connected` is meaningful.
  - For terminal, `connected` may still be `false` even after a successful password submission.
- `select_desktop_display(session, display)`
  - Use on multi-monitor peers before frame capture or clicking.
- `get_desktop_frame(session, display?)`
  - Returns one remote PNG frame plus structured metadata.
  - This is the authoritative visual source for remote desktop automation.
- `lock_remote_user_input(session)`
  - Locks the local `RustDeskMCP` client's human input path for that peer.
  - This is not the old remote-side `block-input` behavior.
- `unlock_remote_user_input(session)`
  - Restores local human input for that peer.
- `mouse_move(session, x, y)`
  - Sends remote pointer movement only.
- `mouse_click(session, x, y, button?, clicks?)`
  - Supported buttons: `left`, `right`, `middle`, `wheel`.
- `keyboard_input(session, text)`
  - Sends text to the remote desktop session.
- `keyboard_hotkey(session, keys)`
  - Uses physical down/up sequencing.
  - Common modifier spellings:
    - `ctrl`, `control`
    - `alt`, `option`
    - `shift`
    - `meta`, `cmd`, `command`, `win`, `windows`, `super`
  - Common control-key spellings:
    - `enter`, `return`
    - `esc`, `escape`
    - `tab`, `space`
    - `backspace`, `delete`, `del`
    - `left`, `right`, `up`, `down`
    - `home`, `end`, `pageup`, `pagedown`
    - `insert`, `capslock`
    - `f1` to `f12`
    - `ctrlaltdel`, `ctrlaltdelete`
    - `lockscreen`
  - Single-character targets are also supported, for example `["win", "r"]`.

### Terminal session tools

- `open_terminal_session(id, rows?, cols?)`
  - Opens a terminal session and currently uses `terminalId = 1`.
  - Returns `session`, `actualSession`, `peerId`, `terminalId`, `needsPassword`, `rows`, `cols`.
- `terminal_input(session, terminalId, data)`
  - Clears the cached terminal output before each input.
  - If `data` ends with `\n`, the tool internally performs the submit. Do not explicitly send an extra `\r` in the normal path.
  - Prefer a single-line shell command string joined with `;`.
- `terminal_output(session, terminalId, limit?)`
  - `output`: ANSI-stripped text
  - `rawOutput`: original decoded output
  - Use explicit markers in commands because shells may echo the submitted command and prompt.

## Desktop workflow notes

- Standard flow:
  1. `open_desktop_session`
  2. `input_password` if `needsPassword`
  3. `select_desktop_display` if needed
  4. `get_desktop_frame`
  5. `lock_remote_user_input`
  6. `mouse_*` and `keyboard_*`
  7. `unlock_remote_user_input`
- The desktop tool descriptions in code still mention temporary blocking, but the actual runtime behavior is explicit lock or unlock only.
- Lock state is keyed by peer id and is cleared when:
  - opening a new session for that peer
  - closing the session
- Remote desktop actions are sent to the remote session, not to the local host. Always validate with `get_desktop_frame`, not with a host screenshot.
- Multi-display handling is explicit:
  - inspect `displays`
  - select a display
  - capture frames from the same display
  - click based on that display's frame

## Terminal workflow notes

- Standard flow:
  1. `open_terminal_session`
  2. `input_password` if `needsPassword`
  3. `terminal_input(..., "<command>\\n")`
  4. poll `terminal_output`
- For Linux targets, the repo now internally converts a trailing newline submit into an asynchronous Enter send. This is required because a single inline send did not behave the same as two separate runtime calls.
- `terminal_output` is easier to parse if the command prints start and end markers, for example:

```sh
printf '__MCP_START__\n'; whoami; hostname; pwd; printf '__MCP_END__\n'
```

- Read-only test commands that were already validated in this repo include:
  - `whoami`
  - `hostname`
  - `pwd`
  - `uname -a`
  - `id`
  - `sed -n '1,8p' /etc/os-release`
  - `ps -eo pid,comm,%cpu,%mem --sort=-%cpu | head -n 8`
  - `df -h | sed -n '1,8p'`

## Known caveats

- Supported MCP surface today is desktop plus terminal only.
- `session` is the MCP-facing handle. `actualSession` is an internal UUID that can change and should not be used as the public handle.
- `get_desktop_frame` can fail transiently if no frame is ready yet. Retry after the session becomes visible.
- `terminal_output` can include the shell's echoed command text and trailing prompt.
- When automating desktop tasks, avoid blind clicks. Fetch a new frame after each meaningful UI change.
