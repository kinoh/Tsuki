# GUI Events History Migration

## Overview
This document records the GUI-side migration from legacy `/messages` history retrieval to `/events`, aligned with the core-rust replacement checklist.

Compatibility Impact: breaking-by-default (GUI now expects event-log contracts from `core-rust`).

## Problem Statement
The GUI chat screen was still coupled to legacy message/thread semantics:

- History fetch used `GET /messages` with `n` and `before`.
- WebSocket receive path expected legacy chat message payloads.
- Client state model stored thread/message-oriented fields (`timestamp`-only pagination assumptions).
- The page expected core-compatible metadata fields, but migration work needed to keep this aligned with core-rust contracts.
- WebSocket path mismatch caused handshake failures (`/ws` in GUI vs `/` in current core-rust router).

This prevented fulfilling the migration goal that `/events` is the only history retrieval API in active clients.

## Solution
The GUI route (`gui/src/routes/+page.svelte`) was updated to consume the event stream contract directly:

- Replace history fetch endpoint with `GET /events?limit=20&order=desc`.
- Replace incremental fetch with `before_ts` pagination.
- Parse WebSocket payload as `{ type: "event", event: Event }`.
- Convert runtime events into UI message items through a dedicated mapping layer.
- Keep `/metadata` probe in connect flow once core-rust exposes the endpoint.

## Design Decisions
1. Event-first state model in GUI
- Why: the backend contract is now event-log based, and thread/message APIs are out of scope.
- Decision: maintain UI-facing `Message` entries, but generate them from `Event` objects.

2. Role mapping from event source/tags
- Why: UI bubble styling still depends on `user|assistant|system` roles.
- Decision: map role using `source` and `meta.tags` (`user_input`, `system_output`), defaulting unknown producers (for example decision/submodule emitters) to `assistant` for readable timeline continuity.

3. Keep optimistic local user message insertion with reconciliation
- Why: immediate input feedback is useful, but websocket echo now returns event rows and can duplicate user messages.
- Decision: insert local temporary items (`localOnly: true`) and replace them when matching user events arrive.

4. Fail-open parsing for chat fragments
- Why: some legacy/imported text may include JSON-like content.
- Decision: attempt JSON parse only for `{...}` text and fall back to plain string when parsing fails.

5. Assistant vs internal message classification
- Why: UI should only treat explicit assistant responses as assistant bubbles; other non-user events are operational/internal outputs.
- Decision: classify events as:
  - `user`: `source == "user"` (or `user_input` tag)
  - `assistant`: `source != "user"` and tags include `response`
  - `internal`: every other `source != "user"` event
- User feedback incorporated: this rule was explicitly requested during migration validation.

## Implementation Details
- Updated imports to remove unused notification symbols.
- Added runtime event types:
  - `RuntimeEvent`
  - `RuntimeEventEnvelope`
  - `EventsResponse`
- Added conversion helpers:
  - `convertEvent`
  - `resolveMessageRole`
  - `parseRuntimeEvent`
  - `upsertRealtimeMessage`
- Replaced `/messages` fetch paths with `/events` paths.
- Replaced numeric `before` pagination with ISO8601 `before_ts`.
- Updated metadata probe handling to parse the core-rust metadata payload.
- Aligned GUI WebSocket URL to the current core-rust route (`/`), fixing handshake 404.
- Updated role mapping so only `response`-tagged non-user events render as assistant messages; other non-user events render as internal messages.

## Future Considerations
- Add explicit filtering rules if UI should hide internal event categories (for example debug/decision-only events).
- Add integration tests that validate:
  - initial `/events` load
  - websocket event rendering
  - load-more pagination via `before_ts`
  - optimistic user message reconciliation
