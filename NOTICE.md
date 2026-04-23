# Notice

This repository is a modified fork of the official [RustDesk](https://github.com/rustdesk/rustdesk) project.

## Upstream project

- Name: RustDesk
- Upstream source: <https://github.com/rustdesk/rustdesk>
- Upstream license: GNU Affero General Public License v3.0
- Local license file: [LICENCE](LICENCE)

## Fork status

This fork keeps the upstream RustDesk codebase as its foundation and adds MCP-related functionality on top of it.

Prominent modifications in this fork include:

- an embedded localhost MCP server inside the Flutter desktop client
- a separate Windows app identity, `RustDeskMCP`, to avoid colliding with a normal RustDesk installation
- MCP desktop-session operations that follow the normal RustDesk connection path
- MCP terminal-session operations
- explicit local input lock and unlock semantics for agent-driven desktop tasks
- remote desktop frame capture for vision-based automation

These modifications were added in this fork in April 2026 and later refined in subsequent commits.

## Attribution and licensing

- Copyright for the original RustDesk project remains with the RustDesk authors and contributors.
- This fork retains the upstream license and upstream notices shipped with RustDesk.
- Modifications in this fork are distributed under the same AGPL-3.0 licensing terms unless a file states otherwise.

## Non-affiliation

This repository is not the canonical RustDesk repository. Unless explicitly stated otherwise, references to RustDesk branding, upstream documentation, release history, and the original project roadmap refer to the upstream RustDesk project and its maintainers.

