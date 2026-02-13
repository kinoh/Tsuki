# Router/Concept-Graph Core-Rust Implementation Log

## Scope
- Implemented the responsibility split from `docs/20260213_router-concept-graph-interface-and-responsibility-clarification.md` in `core-rust`.
- Focused on router output minimization, application-side activation orchestration, and in-process concept-graph interface compatibility.

## Decisions

### 1) Router output was reduced to query terms only
- Changed `RouterOutput` to:
  - `activation_query_terms: Vec<String>`
- Why:
  - Router must remain query-oriented and must not own trigger decisions or concept scoring.
  - This removes implicit policy from router output and aligns with the clarified contract.

### 2) Activation snapshot ownership moved to application orchestration
- Added application-side `ActivationSnapshot` that contains:
  - concept list from concept-graph
  - hard triggers
  - soft recommendations
- Why:
  - Trigger policy belongs to application orchestration, not router.
  - The application now reads concept-graph state and applies configured thresholds.

### 3) Removed embedded module keyword dictionary and ad-hoc concept scoring
- Removed:
  - `module_keywords(...)`
  - weighted token-scoring constants for concept activation
- Replaced with:
  - direct concept-graph read for concept candidates
  - minimal module matching based on module-name presence in input/query/concept names
- Why:
  - Clarification explicitly requires removing embedded keyword heuristics and re-derived concept relevance logic.
  - Keeps activation logic simple and policy-driven by config thresholds.

### 4) Introduced in-process concept-graph trait interfaces
- Added traits in `core-rust/src/activation_concept_graph.rs`:
  - `ConceptGraphActivationReader`
  - `ConceptGraphOps`
  - `ConceptGraphStore` (combined trait)
- Updated `AppState.activation_concept_graph` to `Arc<dyn ConceptGraphStore>`.
- Why:
  - Establishes explicit interface boundaries in core-rust.
  - Keeps activation path in-process while preserving behavior shape compatibility with MCP concept-graph tools.

### 5) Implemented concept-graph ops with MCP-compatible return shape
- Implemented in-process operations:
  - `concept_upsert`
  - `update_affect`
  - `episode_add`
  - `relation_add`
  - `recall_query`
  - `concept_search` (reader API)
- Why:
  - Clarification requires tool-level compatibility and activation read compatibility without MCP round-trips.
  - Result fields and validation outcomes follow existing concept-graph expectations at API shape level.

## Notes
- Current pipeline only consumes `concept_search`; mutation/recall methods are implemented for interface compatibility and future tool integration.
- This implementation intentionally avoids introducing new application-layer semantic scoring heuristics.
