---
date: 2026-02-26
---

# ADR: GUI History Model — Event Stream First

## Context

The GUI was coupled to legacy `/messages` and `/threads` APIs. The backend cutover to `core-rust`
made `/events` the sole history retrieval contract, requiring the GUI state model to follow.

## Decision

- History fetch: `GET /events?limit=20&order=desc` with `before_ts` pagination.
- WebSocket receive: `{ type: "event", event: Event }` payloads.
- UI `Message` entries are generated from `Event` objects via a mapping layer; no thread/message
  model is retained.

### Role mapping

- `user`: `source == "user"` or tag `user_input`.
- `assistant`: `source != "user"` and tags include `response`.
- `internal`: all other non-user events.

Internal events are always collected in memory. A client-only toggle (`showInternalMessages`,
persisted in localStorage) controls rendering — not backend filtering.

### Optimistic user message insertion

Local temporary items (`localOnly: true`) are inserted immediately on send and replaced when the
matching user event arrives via WebSocket echo.

## Rationale

Backend contract is event-log based; thread/message APIs are out of scope. UI bubble roles still
require user/assistant/internal classification, so mapping from `source` and `meta.tags` is the
correct boundary. Keeping internal events in memory (toggle-hidden) lets them remain useful for
debugging without polluting normal conversation view.

## Compatibility Impact

breaking-by-default — GUI now requires event-log contracts from `core-rust`
