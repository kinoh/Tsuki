# Monitoring Console MVP

## Decision
- Keep `GET /debug/ui` focused on existing debug operations and event log.
- Use `GET /debug/monitor` as the operational monitoring surface.
- Monitoring UI must prioritize readability over analysis features.

## Monitoring UX Rules
1. Show newest items at the top.
2. The default visible unit is two lines:
- one `input` line
- one `output` line
3. Do not require expansion-style interaction for normal operation.
4. Treat temporal adjacency as the primary way to interpret event relationships.

## Why
- Operators need fast scanning, not deep trace reconstruction.
- Full payload/event dumps hide important transitions.
- A strict, minimal presentation reduces cognitive load and misreads.

## Scope
- `core-rust/static/monitor_ui.html`
- `core-rust/src/main.rs` (monitor-facing endpoints only)
