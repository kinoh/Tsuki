# Submodule Trigger by Concept Activation

## Context
Recent integration runs showed that submodule trigger recall is effectively zero in practical conversations.
The current trigger path in `core-rust` relies on minimal string matching against submodule names, which does not represent semantic relevance in user input.

There was also terminology drift in prior discussions:
- sometimes "router" meant only the router LLM stage,
- sometimes it meant the full routing path including trigger selection/execution.

Activation concept retrieval itself is already implemented and is not the target of this decision.
The target is trigger policy.

## Decision
Use concept-graph activation of explicit submodule concepts as the only trigger signal.

1. Represent each submodule as a concept node:
- `submodule:curiosity`
- `submodule:self_preservation`
- `submodule:social_approval`

2. Trigger scoring:
- Read activation for each `submodule:*` concept from concept-graph output.
- Use that activation directly for threshold checks.
- No extra heuristic aggregation formula.

3. Trigger policy:
- `hard_trigger` if submodule concept activation `>= hard_trigger_threshold`.
- `soft_recommendation` if submodule concept activation `>= recommendation_threshold`.

4. Remove string-matching trigger logic from runtime trigger selection.

## Explicit Non-Decision
- No fallback path to old string matching.
- If graph relations are missing or activation is low, triggers should not fire.
- This is intentional to expose model/data quality issues clearly in tests.

## Why
- Keeps trigger behavior aligned with concept-graph-centered architecture.
- Avoids hidden behavior from lexical shortcuts.
- Makes integration metrics (especially recall and precision) meaningful and diagnosable.
- Ensures failures are observable instead of being masked by fallback heuristics.

## Validation Direction
After implementation, evaluate with integration scenarios focused on:
- trigger precision,
- trigger recall,
- noninterference,
- flow preservation.

If recall remains low, fix graph representation/relations or prompt policy, not by reintroducing lexical fallback.
