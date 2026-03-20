# Decision: Internal State Tools (In-Memory)

## Context
We need a minimal internal state store to support module-driven tool calls, starting with
an in-memory implementation and a stable interface for later persistence.

## Decision
- Define a `StateStore` interface with `set`, `get`, and `search`.
- Implement `InMemoryStateStore` using an `RwLock<HashMap<...>>`.
- Expose the state store to the model via Response API function tools:
  - `state_set`
  - `state_get`
  - `state_search`

## Rationale
- Keeps the initial API surface small while matching the design doc.
- Tool calls allow the model to read/write state without direct access to internals.
- The in-memory implementation is easy to replace with persistent storage later.

## Consequences
- State is lost when the process exits.
- Tool output is serialized as JSON strings in function outputs.
