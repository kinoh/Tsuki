# Event Stream Event Storming Contract

## Decision
- The `core-rust` event stream is defined as an Event Storming-style domain-event observability channel.
- It does **not** provide strict causal ordering or transactional request/response guarantees.
- Consumers must treat ordering/timing irregularities (delay, duplication, out-of-order visibility) as valid behavior unless a specific endpoint explicitly defines stronger guarantees.

## Why
- Recent integration debugging showed that assuming strict causal order in stream consumption leads to incorrect conclusions and brittle tooling behavior.
- The runtime architecture is event-first and asynchronous; the stream is intended to expose domain facts, not to act as a control-plane synchronization primitive.
- Making this explicit avoids implicit contracts and keeps both runtime and tooling decisions consistent.

## Practical Implication
- Stream consumers (including tests and diagnostics) must not infer strict per-turn causality from adjacent events alone.
- If strict synchronization is needed, it must be provided by an explicit contract outside of the generic event stream.

## Compatibility Impact
- breaking-by-default (no compatibility layer)
