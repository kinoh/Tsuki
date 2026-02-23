# Memory Scope Narrowed to Decision Only

Date: 2026-02-23

## Context
- The previous change (`f95d13b4d097fb8aa67c3e2ad34aaf9b67f6e21b`) already removed `base` from memory-owner scope.
- Follow-up request required applying the same idea to `router` and `submodule`.
- Current prompt content already treats memory as relationship state mainly consumed by the decision layer, while router/submodules are better treated as strategy/heuristic modules.

## Decision
- Require `## Memory` section only for `Decision` prompt overrides.
- Reject `memory_section_update` unless `target=decision`.
- Keep `proposal.target` unchanged (`base|router|decision|submodule:<name>`) because structural prompt edits are still valid for all modules.

## Why
- Router and submodules should stay lightweight and behavior-oriented; forcing memory ownership there couples role logic with long-term state policy.
- Decision is the single integration point for user-facing behavior, so limiting memory writes there reduces ownership ambiguity.
- Narrowing the writable memory surface reduces accidental drift and makes self-improvement outcomes easier to reason about.

## Implementation Notes
- `core-rust/src/prompts.rs`: validation now checks `## Memory` only in `Decision`.
- `core-rust/src/application/improve_service.rs`: memory updates now fail unless target is `decision`.
- `core-rust/config.toml`: self-improvement schema text updated to `memory_section_update.target = decision`.
