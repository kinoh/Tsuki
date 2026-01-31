# Decision: Minimal Rust Core CLI/Event Stream

## Context
We want a minimal implementation that lets us observe the event stream while sending user input.
The goal is to see submodule outputs and the decision module in a live, inspectable flow.

## Decision
- Create a top-level Rust core directory (`core-rust`).
- Emit the internal event stream over WebSocket as JSON with `type: "event"`.
- Use a simple in-memory event store and broadcast every event.
- Simulate two fixed prompt-like submodules and one decision module.
- Reuse the existing `core/scripts/ws_client.js` to send inputs and display event output.

## Rationale
- Keeping a single Event Format internally matches the design document and keeps the flow observable.
- WebSocket `type: "event"` messages make the event stream easy to consume with the existing CLI.
- Stubbed modules allow fast iteration without introducing LLM dependencies yet.

## Consequences
- The Rust core is independent from the current TypeScript core and can evolve safely.
- The event stream is visible immediately but lacks persistence beyond the process lifetime.
- Module behavior is deterministic and intentionally simple for now.
