# Router/Concept-Graph Responsibility Clarification

## Context
After initial router-first activation implementation, several points were clarified:
- Router should not execute tools for activation-path processing.
- Core activation paths must not depend on MCP round-trip latency.
- The application layer should only read current concept-graph state for activation context.
- Event stream already owns execution trace responsibility (including which submodule ran).
- Submodules should be the actor that updates concept-graph relations based on their own purpose.

This document consolidates the agreed direction and defines a concrete in-process concept-graph interface for `core-rust`.

## Final Responsibility Split

### Router
- Input: raw user input text.
- Role: absorb linguistic ambiguity and output query-oriented terms.
- No tool execution in activation path.
- No direct relation updates in activation path.
- No hard/soft trigger decision responsibility.

### Application Orchestrator
- Input: router output (query terms), event history, active submodules.
- Role: read current concept-graph state, decide hard/soft trigger policy, compose decision context, execute hard triggers if configured.
- Must not implement ad-hoc semantic scoring logic beyond minimal shaping needed for prompt context.
- Must not call MCP transport for activation path.

### Decision/Submodules
- Decision decides whether to invoke submodule tools.
- Submodules may update concept-graph (e.g., `relation_add`, `update_affect`) based on submodule purpose.
- Execution provenance is recorded by event stream; no extra ownership transfer is needed.

### Improve Phase
- Improve does not require fixed KPI schema at this stage.
- Improve should use long-horizon event evidence and submodule-intent reasoning rather than immediate per-input heuristics.
- Prompt/update proposals remain reviewed through existing improve pipeline.

## Router Output Contract (Clarified)
Router output should be minimal and query-oriented:

```json
{
  "activation_query_terms": ["..."]
}
```

Notes:
- `activation_query_terms` is required.
- Router generates terms; it does not execute concept-graph mutations.
- Trigger decisions are made outside router.

## In-Process Concept-Graph Interface (Core-side)

The core-side module interface must be MCP-compatible in behavior, but used in-process (no MCP transport dependency in activation path).

### Activation Read Interface (required for application path)

```rust
pub trait ConceptGraphActivationReader {
    async fn concept_search(&self, keywords: &[String], limit: usize) -> Result<Vec<String>, String>;
}
```

Semantics:
- Equivalent to `mcp/concept-graph` `concept_search` behavior:
  - partial name matching first,
  - fallback fill by arousal-ranked concepts.
- `limit` clamp behavior must match the concept-graph contract.

### Mutation/Recall Interface (required for tool-level compatibility)

```rust
pub trait ConceptGraphOps {
    async fn concept_upsert(&self, concept: String) -> Result<serde_json::Value, String>;
    async fn update_affect(&self, target: String, valence_delta: f64) -> Result<serde_json::Value, String>;
    async fn episode_add(&self, summary: String, concepts: Vec<String>) -> Result<serde_json::Value, String>;
    async fn relation_add(&self, from: String, to: String, relation_type: String) -> Result<serde_json::Value, String>;
    async fn recall_query(&self, seeds: Vec<String>, max_hop: u32) -> Result<serde_json::Value, String>;
}
```

Return shape and validation rules must remain equivalent to:
- `mcp/concept-graph/README.md`
- `mcp/concept-graph/src/service.rs`

- `concept_search` is the application-facing read API with no side effects.

## What Should Be Removed from `pipeline_service`
- Embedded module intent keyword dictionary (`module_keywords`) in application orchestration.
- Ad-hoc concept scoring constants that are not part of concept-graph contract.
- Any logic that effectively re-derives concept relevance after concept-graph already ranked candidates.

Why:
- Current embedded heuristics are inferior to the existing core behavior and should be removed.

## What Must Stay in `pipeline_service`
- Router orchestration.
- Activation snapshot retrieval from concept-graph reader.
- Hard/soft trigger decision and execution orchestration.
- Decision context composition.
- Event-stream-based trace consistency.

## Impact on Existing Docs
- This document supersedes ambiguous interpretations in:
  - `docs/20260211_router-concept-activation-submodule-gating.md` (responsibility details)
  - `docs/20260212_router-concept-activation-core-rust-implementation.md` (implementation notes where heuristics were introduced)

## Rationale
- Keeps activation path in-process.
- Preserves compatibility with existing Memgraph concept assets and concept-graph behavior.
- Avoids pushing submodule-purpose logic into application glue code.
