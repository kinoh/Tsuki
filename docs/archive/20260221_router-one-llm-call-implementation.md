# Router One-LLM-Call Implementation Notes

## Context
- The router preconscious design was updated on 2026-02-21 to require a strict one-LLM-call runtime path.
- Previous runtime behavior allowed LLM tool loops (`concept_search` and `recall_query`) inside the router call.

## Decisions
- Moved `concept_search` to router preprocessing (application side), using deterministic query-term extraction (v0).
- Constrained router LLM to seed selection only, with tools disabled at the adapter configuration level.
- Moved `recall_query` to router postprocessing (application side), converting its propositions into the final shared state text.
- Preserved the downstream contract by continuing to emit only `active_concepts_from_concept_graph` as router module output.

## Why
- Prompt-only constraints are not sufficient to guarantee single-call behavior; adapter-level tool disabling is required for deterministic execution.
- Separating preprocessing/LLM/postprocessing improves observability and testability of each stage.
- Keeping output contract unchanged avoids unnecessary integration churn while still applying the new architecture.

## Additional Notes
- `concept_graph.query` debug events now include candidate concepts and selected seeds as intermediate diagnostics.
- Query-term extraction is intentionally simple and deterministic in v0 (normalization, character-class segmentation, positional variants, dedupe/filter, max cap).
