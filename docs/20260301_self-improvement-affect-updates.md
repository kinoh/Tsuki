# Self-Improvement Trigger Plan Supports Affect Updates

## Context
- Trigger processing already handled concept upserts, relation additions, and episode additions.
- Emotional state updates (`update_affect`) were available in concept graph operations but unreachable from self-improvement plans.
- This prevented self-improvement from adjusting valence/arousal state when reflection identified affective drift.

## Decision
- Extend trigger processing plan schema with `affect_updates`:
  - `target: string`
  - `valence_delta: number`
- In `improve_service`, execute each affect update via `activation_concept_graph.update_affect(target, valence_delta)`.
- Mark `concept_graph_updated=true` when affect update succeeds.
- Emit `AFFECT_UPDATE_FAILED` issue code when affect update fails.

## Why
- Makes affect adjustment a first-class self-improvement action.
- Reuses existing concept graph operation instead of adding a parallel mechanism.
- Keeps observability and partial-failure behavior aligned with existing trigger worker semantics.

## Compatibility Impact
breaking-by-default (no compatibility layer)

