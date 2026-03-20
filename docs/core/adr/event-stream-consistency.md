---
date: 2026-02-21
---

# ADR: Event Stream Eventual Consistency Contract

## Decision

The `core-rust` event stream is an Event Storming-style domain-event observability channel.
It does **not** provide strict causal ordering or transactional request/response guarantees.
Consumers must treat ordering/timing irregularities (delay, duplication, out-of-order visibility)
as valid behavior unless a specific endpoint explicitly defines stronger guarantees.

## Rationale

The runtime architecture is event-first and asynchronous. The stream exposes domain facts, not
control-plane synchronization primitives. Making this explicit prevents implicit contracts from
forming in consumers and keeps both runtime and tooling decisions consistent.

Integration debugging showed that assuming strict causal order leads to incorrect conclusions and
brittle tooling behavior.

## Practical Implication

- Stream consumers (including tests and diagnostics) must not infer strict per-turn causality from
  adjacent events alone.
- If strict synchronization is needed, it must be provided by an explicit contract outside of the
  generic event stream.

## Compatibility Impact

breaking-by-default (no compatibility layer)
