# Decision: Dynamic Submodules, Event History, Question Events

## Context
The Rust core needs dynamic submodules, a decision module that reads from the event store, and
explicit question events when the decision output requests clarification.

## Decision
- Add an in-memory `ModuleRegistry` to manage submodule definitions and enabled state.
- Build submodule adapters from registry entries at runtime.
- Have the decision module read recent events from the event store (no summary).
- Emit a `question` event when the decision output includes `question=<text>` or `decision=question`.

## Rationale
- The registry makes dynamic module management possible without hard-coding modules.
- Decision-by-history keeps the system observable and consistent with the event-first design.
- Question events preserve uncertainty explicitly in the event stream.

## Consequences
- The registry is in-memory and resets on restart.
- Decision prompts now include recent event history lines.
- Question events are tagged and can be routed to the user interface.
