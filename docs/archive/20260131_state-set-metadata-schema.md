# Decision: Add explicit empty metadata schema

## Context
The Responses API still rejected the `state_set` schema, reporting that `metadata` was an extra required key.
This suggested the `metadata` property was not recognized as a valid object schema.

## Decision
- Define `metadata` with an explicit empty `properties` object and `additionalProperties: false`.

## Rationale
- Some validators require object schemas to include `properties` to be considered valid.
- This keeps `metadata` strict while satisfying schema validation.

## Consequences
- Callers still must pass `metadata: {}` due to the required list.
