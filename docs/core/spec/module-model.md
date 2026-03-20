# Module Model

## Event Stream

The event stream is the central artifact of the runtime. It is an append-only log of domain facts.

- Modules read from the event stream to form context.
- Modules write to the event stream to record what happened.
- The event stream is an observability channel, not a control bus. It does not guarantee causal
  ordering or synchronization between modules (see `adr/event-stream-consistency.md`).

## Modules

A module is an autonomous unit that reads events and emits events. Modules do not communicate with
each other directly — there is no contract between modules. Coordination happens only through the
shared event stream.

This means:
- Adding or removing a module does not require changing other modules.
- A module cannot assume anything about what another module has done or will do.
- Module behavior is defined entirely by which events it reads and which events it emits.

## Module Roles

### Router

Reads incoming input events. Queries the concept graph for activated concepts, skills, and
episodes. Emits an activation snapshot event for the current turn.

Role in event terms: **input → activation state**.

Router does not decide how to respond. It does not know Decision exists.

Repeated or familiar inputs may still activate and surface new facets — strict deduplication is
intentionally avoided to preserve liveliness.

### Submodules

Each submodule reads the current event context through its own motivational lens and emits a
suggestion event. The three built-in motives are:
- `curiosity` — maximize learning and feedback opportunities.
- `self_preservation` — prioritize stable operation and risk reduction.
- `social_approval` — improve perceived helpfulness and rapport.

Role in event terms: **context → motive suggestion**.

Submodules do not communicate with each other. A submodule does not know whether its suggestion
will be used.

### Decision

Reads recent event history — including activation state and any motive suggestions present — and
emits a response event. Owns the respond/ignore choice and tool usage.

Role in event terms: **event history → response**.

Decision does not re-derive activation; it consumes what is already in the event stream.
Memory (the `## Memory` prompt section) belongs to Decision only, because it is the sole
integration point for long-term user-facing behavior.

## Extension Guidance

- New modules should be defined by their event read/write contract, not by their position in a
  sequence.
- If a new module needs output from another module, read it from the event stream — do not add a
  direct call.
- Module count and composition can change without touching other modules or the event stream schema.
