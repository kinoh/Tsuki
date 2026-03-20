# GUI — Overview

The GUI is a Tauri + Svelte desktop/mobile client. It communicates with the core backend over
WebSocket (real-time events) and HTTP (history, config, metadata).

## Functional Areas

**Chat view** — displays the conversation as a timeline of events. History is fetched from
`GET /events` and paginated with `before_ts`. New events arrive over WebSocket as
`{ type: "event", event: Event }`. Internal events (non-user, non-response) are collected but
hidden by default behind a client-side toggle.

**Config** — reads and writes runtime config via `GET/PUT /config`. Settings are persisted
server-side.

**Status / logs** — local log buffer stored in localStorage. Sensitive fields (tokens,
authorization) are masked before storage. Logs are visible in the Status overlay with regex
filtering.

## Key Constraints

- History model is event-stream first: no thread/message APIs.
- Role mapping uses `source` and `meta.tags` (`user_input`, `response`) — not a separate role
  field.
- Internal event visibility is a client-only concern; backend filtering is not used for this.
- WebSocket connects to `/`; auth frame must be sent immediately after connection.
