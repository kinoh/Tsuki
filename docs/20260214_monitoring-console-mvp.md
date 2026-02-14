# Monitoring Console MVP

## Decision
- Keep `GET /debug/ui` focused on existing debug operations and event log.
- Add a separate monitoring page at `GET /debug/monitor`.
- Provide monitoring APIs:
  - `GET /debug/decision-traces`
  - `GET /debug/concepts/query`

## Why
- Monitoring and debug editing have different responsibilities.
- A dedicated monitor reduces operational noise and keeps debug UI stable.

## Implemented Scope
- Backend routes and handlers in `core-rust/src/main.rs`.
- New monitoring page in `core-rust/static/monitor_ui.html`.

## MVP Coverage
1. Live stream monitor via `/debug/events/stream`.
2. Decision trace view grouped by user input turns.
3. Concept inspector using concept search and recall query.
