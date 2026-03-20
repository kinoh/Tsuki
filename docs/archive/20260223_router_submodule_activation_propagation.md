# Router Submodule Activation Propagation

## Context
- Submodule trigger scores were nearly flat across turns (`~0.476`) and mostly reflected recency decay from `submodule:*` upsert state.
- The expected behavior is: active front concepts in the current turn should raise related submodule activation in that same turn.

## Issue
- Router score calculation reads `concept_activation("submodule:*")`, but no turn-time propagation from selected front concepts to submodule concepts was executed.
- As a result, hard/soft trigger decisions did not reflect current user-input semantics.

## Decision
- Add router-time propagation before threshold selection:
  - take router-selected seed concepts (excluding `submodule:*`),
  - propagate activation to related `submodule:*` concepts through graph relations,
  - then compute module scores and hard/soft trigger decisions.

## Why
- This preserves concept-graph-first trigger semantics.
- It reduces dependence on stale recency values.
- It aligns runtime behavior with the intended interpretation of concept links.
