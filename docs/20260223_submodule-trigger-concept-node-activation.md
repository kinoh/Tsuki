# Submodule Trigger Uses Concept Node Activation Only

## Context
A recent integration result showed low submodule trigger recall in a curiosity-focused scenario.
During fix discussion, one candidate approach was to derive trigger scores from `recall_query` propositions where `submodule:*` appears as an endpoint.

The user clarified this is not the intended contract.
The trigger decision must use the activation of the `Concept` node itself, not relation proposition scores.

## Decision
For runtime submodule trigger scoring in `core-rust`:
- Use `submodule:<name>` concept node activation as the only signal.
- Do not derive trigger scores from proposition text or relation endpoints.
- Keep threshold checks unchanged:
  - `hard_trigger` if activation `>= hard_trigger_threshold`
  - `soft_recommendation` if activation `>= recommendation_threshold`
- Keep no-fallback behavior: missing/low activation means no trigger.

## Implementation Notes
- Added `concept_activation(concepts)` to `ConceptGraphActivationReader`.
- Implemented concept activation lookup in `ActivationConceptGraphStore` using the same arousal decay model (`arousal_level`, `accessed_at`) as runtime concept state.
- Updated router scoring to map module names to `submodule:<name>` concept activation.
- Exposed `module_scores` in router output for observability.

## Why
- Matches the previously agreed concept-graph-first trigger policy.
- Avoids accidental semantic drift between "concept activation" and "relation recall".
- Keeps failures diagnosable as graph quality / activation quality issues instead of text parsing artifacts.
