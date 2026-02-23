# Suppress Arousal Self-Sustain for Submodule Concept Nodes

## Context
Submodule trigger scoring now reads activation directly from `submodule:<name>` concept nodes.
In integration behavior, submodule triggers tended to stay high across turns even when immediate user intent should have narrowed.

The likely cause was that `recall_query` arousal updates also applied to `submodule:*` nodes, which allowed trigger nodes themselves to self-sustain activation over turns.

## Decision
In `ActivationConceptGraphStore::recall_query`, do not apply `maybe_update_arousal` to targets whose concept name starts with `submodule:`.

- Keep arousal update behavior unchanged for non-submodule concept nodes.
- Keep trigger scoring contract unchanged (`concept_activation(submodule:*)` + thresholds).

## Why
- Reduce unintended cross-turn persistence of submodule trigger activation.
- Preserve concept-graph-driven trigger policy while preventing trigger nodes from acting as self-reinforcing memory.
- Keep change surface minimal and diagnosable.
