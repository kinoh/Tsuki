# Decision: WebSocket auth/connection logs

## Context
We want stdout logs to reflect connection and authentication flow, not only module execution.

## Decision
- Emit logs for WebSocket connect, auth success/failure, client close, send failures, and disconnect.
- Keep log lines concise and single-line to preserve readability in terminal streams.

## Rationale
- Operators need to see connection lifecycle and auth issues without reading the event stream.
- These logs complement the event stream rather than duplicating its content.

## Consequences
- Additional stdout noise, but improves observability during development.
