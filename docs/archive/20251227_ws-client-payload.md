# Decision: Align GUI WebSocket payload to AsyncAPI

Date: 2025-12-27

## Context
The GUI WebSocket client was sending raw text, while `api-specs/asyncapi.yaml` defines a JSON payload with a required `type` field.

## Decision
Send a JSON payload with `{ type: "message", text }` from the GUI. Image support and WebSocket auth framing are deferred.

## Consequences
The GUI now matches the AsyncAPI client message shape for text messages. Further changes are needed if image payloads or auth frames are introduced in the spec.
