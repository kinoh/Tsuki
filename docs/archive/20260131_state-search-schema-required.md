# Decision: Require limit in state_search schema

## Context
The Responses API rejected the `state_search` schema because `required` must include every key
in `properties`. The error reported missing `limit`.

## Decision
- Update `state_search` so `required` includes both `query` and `limit`.

## Rationale
- The function schema validator expects all properties to be listed in `required`.
- This avoids repeated invalid schema errors during tool registration.

## Consequences
- Callers must always supply `limit` explicitly.
