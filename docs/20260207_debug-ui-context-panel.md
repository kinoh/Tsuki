# Debug UI Context Panel

## Context
- `llm.raw` payload includes the full composed `context` sent to the LLM.
- The UI only displayed raw response JSON, making input-side inspection difficult.

## Decision
- Add an `LLM Input Context` panel above `Raw Response`.
- Populate it from `llm.raw.payload.context`.

## Why
- Input/output observability should be available in one screen.
- Helps validate history cutoff/exclusion effects directly.

## Implementation Notes
- On Work Log selection, context is loaded with the matching `llm.raw` event.
- On `Load Raw`, context and raw are updated together.
- For non-module rows (e.g. reply), context shows `(none)`.
