# RustDeskMCP

RustDeskMCP is a modified RustDesk fork that embeds a local MCP server into the desktop Flutter client so an AI agent can drive RustDesk through explicit MCP tools instead of only through direct human GUI interaction.

## Upstream basis

- Upstream project: [RustDesk](https://github.com/rustdesk/rustdesk)
- Upstream license: AGPL-3.0
- Fork notice and attribution: [../NOTICE.md](../NOTICE.md)

This repository intentionally preserves the upstream RustDesk codebase and layers MCP behavior on top of it instead of replacing the normal product flow.

## Design goals

- Preserve the normal non-MCP RustDesk user path.
- Expose MCP only when the user enables it from `Settings -> Security -> Enable MCP server`.
- Keep this fork separate from a normal RustDesk installation by using the `RustDeskMCP` app identity.
- Make MCP actions follow the same user-visible connection path as RustDesk sessions.
- Support remote vision-driven operation by exposing actual remote desktop frames.

## Current MCP scope

### Desktop session

Implemented desktop MCP tools:

- `open_desktop_session`
- `input_password`
- `select_desktop_display`
- `get_desktop_frame`
- `lock_remote_user_input`
- `unlock_remote_user_input`
- `mouse_move`
- `mouse_click`
- `keyboard_input`
- `keyboard_hotkey`

Current desktop behavior:

- desktop sessions open through the normal RustDesk connection path
- multi-display selection is explicit
- frame capture returns a remote desktop frame for the selected display
- local human input through `RustDeskMCP` can be explicitly dropped while MCP actions continue through a bypass path

### Terminal session

Implemented terminal MCP tools:

- `open_terminal_session`
- `input_password`
- `terminal_input`
- `terminal_output`

Current terminal behavior:

- terminal sessions open through the normal RustDesk terminal path
- terminal output is cached and can be read back through MCP
- a command ending with `\n` is internally submitted by `terminal_input`
- `terminal_output.output` is ANSI-stripped and `terminal_output.rawOutput` preserves the raw decoded stream

### Not implemented yet

This fork does not yet expose MCP support for:

- camera sessions
- printer flows
- other RustDesk session types outside desktop and terminal

## Runtime model

### App identity

On Windows, this fork runs as `RustDeskMCP` so it does not steal focus from or collide with a normal RustDesk instance on the same machine.

### MCP enablement

The supported enablement path is the GUI setting:

- `Settings -> Security -> Enable MCP server`

This fork does not rely on a dedicated `--mcp` startup mode for normal usage.

### Local endpoint

The built-in server listens on:

- `http://127.0.0.1:59940/mcp`

The transport is JSON-RPC 2.0 over HTTP `POST`.

## Recommended workflow

### Desktop workflow

1. Call `open_desktop_session(id)`.
2. If `needsPassword` is true, call `input_password(session, password)`.
3. If the peer has multiple displays, inspect `displays` and call `select_desktop_display`.
4. Call `get_desktop_frame` before acting.
5. For a multi-step automation task, call `lock_remote_user_input` once at the start.
6. Send mouse and keyboard operations.
7. Fetch another desktop frame after meaningful UI changes.
8. Call `unlock_remote_user_input` when the task finishes.

### Terminal workflow

1. Call `open_terminal_session(id, rows?, cols?)`.
2. If `needsPassword` is true, call `input_password(session, password)`.
3. Send a command through `terminal_input(session, terminalId, "<command>\n")`.
4. Poll `terminal_output`.
5. Prefer explicit output markers when the command must be machine-read.

## Important caveats

- `session` is the MCP-facing handle. `actualSession` is diagnostic and should not be treated as the public handle.
- `input_password` may still report `connected: false` for terminal sessions even when the password path was accepted. Terminal readiness should be verified by sending input and reading output.
- `get_desktop_frame` is the authoritative source for remote vision automation. A screenshot of the local host desktop is not an acceptable substitute.
- The input lock is local to the `RustDeskMCP` client and drops human input sent through that client. It is not the old remote-side `block-input` behavior.

## Related repo assets

- Agent-oriented skill: [../skills/rustdesk-mcp-operator/SKILL.md](../skills/rustdesk-mcp-operator/SKILL.md)
- More detailed MCP workflow notes: [../skills/rustdesk-mcp-operator/references/mcp-workflows.md](../skills/rustdesk-mcp-operator/references/mcp-workflows.md)
