# Monitoring Console MVP

## Decision
- Keep `GET /debug/ui` focused on existing debug operations and event log.
- Use `GET /debug/monitor` as the operational monitoring surface.
- Monitoring UI must prioritize readability over analysis features.
- Event metadata semantics must stay stable and unambiguous.

## Metadata Semantics
1. `source` is reserved for module ownership only.
- Allowed intent:
  - `user`
  - `router`
  - `decision`
  - `submodule:<name>`
  - `system` (only for events with no module ownership)
- Do not encode non-module classifications into `source`.

2. Non-module classifications are expressed via tags/payload.
- `llm.raw` is represented by `tag=llm.raw`, not by `source=llm_raw`.
- `concept_graph.query` is represented by `tag=concept_graph.query`.
- reply/action semantics are represented by tags such as `action` and `response`.

3. Interpretation rule for monitor UI.
- Timeline primary axis:
  - ownership = `source`
  - meaning/details = `tags` and `payload`
- UI must not reinterpret `source` into mixed semantic categories.

4. Emission rule for runtime.
- `source` must be assigned explicitly at each event emission site.
- Runtime must not infer ownership from tags/payload as a post-hoc normalization step.
- Runtime should not duplicate ownership as `payload.module` or `tag=module:*`.

## Functional Scope
1. Live timeline view
- Show newest items at the top.
- Keep receiving new events in real time.

2. Event-native timeline view (default)
- The default visible unit is one event line.
- Do not collapse multiple events into synthetic input/output pairs.
- Prioritize short summaries over full payload rendering.

3. Minimal filtering
- Support only operational filters required for scanning:
  - `source`
  - simple text match
- Do not expose a separate `module` filter or `module` detail field in monitor UI.

4. Lightweight details (optional)
- Normal operation must not depend on expansion-style interaction.
- Detailed payload is shown in a dedicated right-side detail panel.

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
  - `source`: module ownership label
  - `ts`: event timestamp
  - `summary`: payload text or compact payload representation
- Keep rendering stable under continuous updates.
- Avoid mixing monitoring output with debug-editing controls.
- Timeline remains compact while detail inspection is done on the right panel.

## Right Detail Panel
- Selecting a timeline line updates the right panel.
- Required fields:
  - `event_id`
  - `ts`
  - `source`
  - `tags`
  - `payload` (formatted JSON)
- The right panel is for inspection only; no trace reconstruction controls.

## Interaction Model
- Open monitor page and start receiving live events immediately.
- Keep newest entries pinned at the top.
- Apply filters without switching away from timeline mode.
- Do not enforce trace-specific workflows for baseline monitoring.

## Failure and Reconnect Behavior
- Retry SSE connection automatically.
- Keep already loaded timeline items when reconnecting.
- Emit a visible in-timeline error row only when disconnect/reconnect issues are detected.

## Out of Scope (MVP)
- Strict causal graph reconstruction.
- Turn/session IDs added only for monitoring purposes.
- Expansion-first trace analysis as a default workflow.

## Scope
- `core-rust/static/monitor_ui.html`
- `core-rust/src/main.rs` (monitor-facing endpoints only)
