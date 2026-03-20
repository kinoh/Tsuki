# WebSocket chat payload normalization

## Context
- GUI log showed a single `chat` element containing multiple concatenated JSON objects, which prevented UI parsing.
- The WebSocket payload is produced in `core/src/agent/mastraResponder.ts` using `response.text`.

## Decision
- Split concatenated JSON objects in `response.text` into multiple `chat` elements when the text is composed solely of adjacent JSON objects.
- If parsing fails or extra non-JSON text exists, keep the original single string to avoid unintended changes.

## Notes
- This change targets the WS payload only; stored message history remains unchanged.
- Requested by user: ensure each `chat` element is a single JSON object or plain string.
