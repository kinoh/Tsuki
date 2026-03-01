# Self-Improvement Trigger Plan Supports Episode Additions

## Context
- The self-improvement worker plan already supported concept upserts and relation additions.
- It did not support explicit episode creation, so prompt-level guidance about episodes could not be materialized by runtime behavior.
- This mismatch made concept graph updates incomplete for conversational memories that should be represented as episodes.

## Decision
- Extend trigger processing plan schema with `episode_additions`:
  - `summary: string`
  - `concepts: string[]`
- In `improve_service`, execute each episode addition via `activation_concept_graph.episode_add(summary, concepts)`.
- Mark `concept_graph_updated=true` when episode creation succeeds.
- Emit `EPISODE_ADD_FAILED` issue code when episode creation fails.

## Why
- Aligns runtime capability with self-improvement prompt intent.
- Keeps episode creation in the same trigger workflow as concept/relation updates.
- Preserves existing fail-fast and observability semantics for module processing.

## Compatibility Impact
breaking-by-default (no compatibility layer)

