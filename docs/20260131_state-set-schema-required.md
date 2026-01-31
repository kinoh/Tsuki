# Decision: Require all state_set properties

## Context
The Responses API rejected the `state_set` schema because the `required` array must include
every property defined in `properties`. The error reported missing `related_keys`.

## Decision
- Update `state_set` so `required` includes `key`, `content`, `related_keys`, and `metadata`.

## Rationale
- The Responses API validation expects `required` to list all properties for function schemas.
- Keeping the schema strict prevents repeated invalid schema errors during tool registration.

## Consequences
- Callers must always include `related_keys` (can be empty) and `metadata` (can be `{}`).
