---
date: 2026-03-02
---

# ADR: Event Constructor Sealing

## Context

Runtime code used `build_event(...)` directly across modules. Contract drift could only be caught
by review or CI — not by the compiler.

## Decision

- `build_event` is private inside the `event` module.
- `event::contracts` is the only event construction surface.
- `Event` has a private sealed field, making external struct-literal construction impossible.
- DB rehydration uses an explicit `event::rehydrate_event(...)` path.

## Rationale

Enforces event emission through contract-level constructors at compile time. Eliminates accidental
uncontracted event emission without relying on lint or grep rules. Runtime emission logic stays in
application modules; envelope and tag construction is centralized.

## Compatibility Impact

breaking-by-default (no compatibility layer)
