# Activation Concept Search Replacement

## Context
`compute_concept_activation` in `core-rust` used an ad-hoc scoring approach over `state_records` in LibSQL. This diverged from the concept-graph runtime model and did not reuse Memgraph concept assets.

## Decision
- Replaced activation concept lookup in `compute_concept_activation` with direct Memgraph access.
- Added `ActivationConceptGraphStore` (`core-rust/src/activation_concept_graph.rs`) using `neo4rs`.
- Implemented concept search behavior aligned with `mcp/concept-graph` semantics:
  - keyword partial match first,
  - fallback fill by arousal ranking with exponential decay.
- Wired the store into `AppState` and switched router activation path to call this store.

## Why
- Activation path should use concept-graph assets directly, not a separate local text store.
- This removes the prior ad-hoc dependence on `state_records` for router activation.
- Keeps activation in-process with no MCP round-trip in the critical path.

## Notes
- Current change focuses on replacing activation lookup/scoring source.
- Full tool-level compatibility with `mcp/concept-graph` APIs is a separate step.
