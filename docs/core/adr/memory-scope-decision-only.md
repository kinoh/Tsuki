---
date: 2026-02-23
---

# ADR: Memory Section Scope Narrowed to Decision Only

## Context

`## Memory` sections in prompt overrides were accepted for all module targets (base, router,
decision, submodule). Router and submodules are behavioral/heuristic modules; coupling them to
long-term memory state creates ownership ambiguity and makes self-improvement outcomes harder to
reason about.

## Decision

- `## Memory` sections are only valid in `Decision` prompt overrides.
- `memory_section_update` is rejected unless `target=decision`.
- Structural prompt edits (`proposal.target`) remain valid for all modules.

## Rationale

Decision is the single integration point for user-facing behavior, so memory ownership there is
natural and unambiguous. Router and submodules should stay lightweight and behavior-oriented.
Narrowing the writable memory surface reduces accidental drift.

## Compatibility Impact

breaking-by-default (no compatibility layer)
