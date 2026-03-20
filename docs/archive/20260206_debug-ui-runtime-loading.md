# Debug UI Runtime Loading

## Context
- Editing `core-rust/static/debug_ui.html` required restarting `core-rust` because the UI was served via compile-time `include_str!`.
- The developer workflow needs immediate reflection of UI edits without process restart.

## Decision
- Serve `/debug/ui` by reading `core-rust/static/debug_ui.html` at request time.
- Keep an embedded fallback (`include_str!`) for failure cases so debug UI remains available if file reading fails.

## Why
- Runtime file loading removes a dev-time restart loop and accelerates UI iteration.
- Embedded fallback keeps behavior resilient for packaged or constrained environments.

## Implementation Notes
- `debug_ui` now returns `Html<String>` instead of `Html<&'static str>`.
- Path is resolved via `concat!(env!("CARGO_MANIFEST_DIR"), "/static/debug_ui.html")`.
- On read error, the server logs `DEBUG_UI_READ_ERROR` and responds with embedded HTML.
