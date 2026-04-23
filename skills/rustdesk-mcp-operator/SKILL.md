---
name: rustdesk-mcp-operator
description: Operate the RustDeskMCP desktop and terminal MCP bridge in this repository. Use when Codex needs to launch or verify the local RustDeskMCP app, enable the built-in MCP server from the Security settings page, connect to a RustDesk desktop or terminal session, provide connection passwords, select a remote display, fetch remote desktop frames for vision, explicitly lock or unlock local RustDeskMCP user input, send remote mouse or keyboard actions, or run read-only terminal commands through the repo's localhost MCP server.
---

# RustDesk MCP Operator

## Overview

Use this skill for the MCP features implemented in this repository, not for the separately installed RustDesk client. The supported MCP surface is desktop session control plus terminal session control, both served by the local `RustDeskMCP` process over `http://127.0.0.1:59940/mcp`.

## Quick Start

- Launch the repo-local `test-runtime/RustDeskMCP/RustDeskMCP.exe` package or the current repo build. Do not rely on `rustdesk.exe --mcp`; the supported path is the normal GUI executable plus the settings toggle.
- In the GUI, enable `Settings -> Security -> Enable MCP server`.
- Use JSON-RPC over `POST /mcp`: `initialize`, then `tools/list` or `tools/call`.
- Treat the returned `session` field as the MCP handle for follow-up calls. `actualSession` is diagnostic only.

## Desktop Workflow

1. Call `open_desktop_session(id)`.
2. If `needsPassword` is true, call `input_password(session, password)`.
3. Inspect `displays`. If the peer is multi-monitor, call `select_desktop_display(session, display)` before acting.
4. Call `get_desktop_frame(session, display?)` and reason from the returned remote frame, not from a local host screenshot.
5. For a multi-step control task, call `lock_remote_user_input(session)` once before the task and `unlock_remote_user_input(session)` once after the task.
6. Use `mouse_move`, `mouse_click`, `keyboard_input`, and `keyboard_hotkey` to drive the remote UI.

## Terminal Workflow

1. Call `open_terminal_session(id, rows?, cols?)`.
2. If `needsPassword` is true, call `input_password(session, password)`.
3. Send a shell command through `terminal_input(session, terminalId, data)`.
4. End the command string with a single trailing `\n` when it should execute immediately. The tool now performs the submit internally.
5. Read results through `terminal_output(session, terminalId, limit?)`.

## Important Rules

- Only desktop and terminal sessions are implemented. Camera, printer, file-transfer, and other session types are not exposed through MCP.
- The desktop mouse and keyboard tools do not implicitly lock the user anymore. The explicit `lock_remote_user_input` and `unlock_remote_user_input` calls are the intended control path.
- The lock is local to the `RustDeskMCP` client. It drops human input sent through that client while allowing MCP actions through a bypass path.
- Session open and session close clear stale lock state for that peer. Still unlock explicitly when finishing a task.
- Prefer `get_desktop_frame` before and after desktop actions, especially on multi-display peers.
- For terminal usage, prefer a single-line shell command joined with `;` and explicit output markers. This avoids shell paste edge cases and makes `terminal_output` easier to parse.
- `terminal_output.output` is ANSI-stripped. `terminal_output.rawOutput` keeps the raw terminal text.
- `input_password` may still report `connected: false` for terminal sessions even when the password was accepted. Verify terminal readiness with `terminal_input` plus `terminal_output`.

## Read More

- Read [references/mcp-workflows.md](references/mcp-workflows.md) for exact tool names, arguments, return semantics, hotkey spellings, runtime paths, build commands, and known caveats.
