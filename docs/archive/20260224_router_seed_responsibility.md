# Router Seed Responsibility Clarification

## Decision
Removed downstream seed relevance filtering logic from `router_service` and restored single ownership of seed selection to the router LLM output.
Also defined a strict seed-selection invariant for developers and router prompts.

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
- Added router instruction invariant in `core-rust/config.toml`:
  - seeds must be relevant to latest user utterance
  - seeds must be worth activating now
  - if none exists, return `none`
- Unified router prompt shape under config template ownership:
  - `router_preprocessing` and `seed_selection_rules` sections are now injected through
    `input.router_context_template` instead of hardcoded string concatenation in code.
  - `seed_selection_rules` text itself is defined in config template (not in Rust source).

## Effect
Seed activation now depends only on router-selected seeds, with no additional downstream relevance gate.

## Developer invariant
When changing router/concept-activation code, keep this invariant:
- Do not introduce downstream reinterpretation of seed relevance.
- Do not auto-backfill seeds from arousal-ranked concepts.
- Router output is the only source of seed truth, and it must already satisfy
  "relevant to current utterance" + "activate now" constraints.
