# Monitoring Console MVP

## Decision
- Keep `GET /debug/ui` focused on existing debug operations and event log.
- Use `GET /debug/monitor` as the operational monitoring surface.
- Monitoring UI must prioritize readability over analysis features.

## Functional Scope
1. Live timeline view
- Show newest items at the top.
- Keep receiving new events in real time.

2. Input/Output pair view (default)
- The default visible unit is two lines:
  - one `input` line
  - one `output` line
- Prioritize short summaries over full payload rendering.

3. Minimal filtering
- Support only operational filters required for scanning:
  - `source`
  - `module`
  - simple text match

4. Lightweight details (optional)
- Normal operation must not depend on expansion-style interaction.
- Detailed payload can be shown only as secondary information.

## Why
- Operators need fast scanning, not deep trace reconstruction.
- Full payload/event dumps hide important transitions.
- A strict, minimal presentation reduces cognitive load and misreads.
- Temporal adjacency is sufficient for practical monitoring in this project.

## Data Sources and Endpoints
- Live updates: `GET /debug/events/stream` (SSE).
- History and filtered fetch: `GET /debug/events`.
- Monitor page entrypoint: `GET /debug/monitor`.

## Default Timeline Presentation
- Primary line format:
  - `input`: timestamp + user text summary
  - `output`: timestamp + assistant/system output summary
- Keep rendering stable under continuous updates.
- Avoid mixing monitoring output with debug-editing controls.

## Interaction Model
- Open monitor page and start receiving live events immediately.
- Keep newest entries pinned at the top.
- Apply filters without switching away from timeline mode.
- Do not enforce trace-specific workflows for baseline monitoring.

## Failure and Reconnect Behavior
- Show current stream state (`connected`, `reconnecting`, `disconnected`).
- Retry SSE connection automatically.
- Keep already loaded timeline items when reconnecting.

## Out of Scope (MVP)
- Strict causal graph reconstruction.
- Turn/session IDs added only for monitoring purposes.
- Expansion-first trace analysis as a default workflow.

## Scope
- `core-rust/static/monitor_ui.html`
- `core-rust/src/main.rs` (monitor-facing endpoints only)
