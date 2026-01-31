# Decision: Fix state_set JSON Schema

## Context
Function tool calls failed with `invalid_function_parameters` because the `metadata` field in
`state_set` lacked an explicit `additionalProperties: false` entry.

## Decision
- Update the `state_set` tool schema to set `metadata` as an object with
  `additionalProperties: false`.

## Rationale
- The Responses API requires `additionalProperties` to be specified and set to false for object properties.
- Keeping the schema strict prevents unexpected keys and avoids tool call validation errors.

## Consequences
- The `metadata` object is now strict and must only contain explicitly defined keys
  (currently none), so callers should either omit it or include an empty object.
