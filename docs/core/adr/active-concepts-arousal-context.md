---
date: 2026-03-07
---

# ADR: Active Concepts Context — Arousal Snapshot, Not Relation Text

## Context

The shared state passed to Decision and submodules was labeled `active_concepts_from_concept_graph`
but carried proposition lines (`A evokes B  score=...`). This leaked graph relation structure into
downstream prompts and duplicated capability information already provided by MCP tool contracts.

## Decision

- Replace relation-text payload with a global active-node snapshot taken after router activation.
- Format: `label + arousal` lines, ordered by arousal descending.
- Context key: `active_concepts_and_arousal`.
- All node kinds (concepts, episodes) are included. Episode nodes render with `summary` when
  available.
- Capability exposure remains the responsibility of visible MCP tool contracts, not concept-graph
  relation text.

## Rationale

Downstream modules need to know *what is currently active and how strongly*, not *how nodes relate
to each other*. Relation direction/type is router-internal and adds noise to decision context.

## Compatibility Impact

breaking-by-default (no compatibility layer)
