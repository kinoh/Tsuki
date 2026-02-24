# Router Seed Responsibility Clarification

## Decision
Removed downstream seed relevance filtering logic from `router_service` and restored single ownership of seed selection to the router LLM output.

## Why
A local relevance-scoring filter was introduced to block non-conversational seed reuse. This conflicted with the intended router model:
- router should tolerate ambiguity and decide seeds directly
- speed is prioritized in router stage
- downstream modules should not reinterpret router intent

The user explicitly required that there be no secondary relevance judgement after router seed selection.

## Change
- In `core-rust/src/application/router_service.rs`:
  - removed `filter_seeds_by_conversation_relevance(...)`
  - removed `seed_conversation_relevance_score(...)`
  - removed post-parse filtering call
- Kept no-fallback behavior: when router returns no seeds, no automatic arousal-ranked backfill is applied.

## Effect
Seed activation now depends only on router-selected seeds, with no additional downstream relevance gate.
