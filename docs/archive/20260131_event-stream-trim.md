# Decision: Remove submodule input events from the stream

## Context
The event stream exists primarily for the decision module. Submodule input prompts
do not contribute useful signal to decisions and add noise.

## Decision
- Do not emit submodule input as events.
- Keep submodule outputs, decision, question, action, tool results, and errors in the stream.
- Log submodule input to stdout instead for operational visibility.

## Rationale
- The decision module needs concise, relevant events.
- Removing prompt inputs reduces noise and token pressure in history.

## Consequences
- Prompt inputs are no longer visible in the event stream; they are only visible in stdout logs.
