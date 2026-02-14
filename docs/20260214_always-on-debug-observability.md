# Decision: Always-On Debug Observability Events

## Context
- Runtime observability events (`debug,llm.raw`, `debug,llm.error`, `debug,concept_graph.query`) were discussed as debug-oriented.
- Product requirement changed: these events must be available at all times, not only when a debug-oriented execution path is used.
- Existing history assembly already excludes debug-tagged events via `is_debug_event`, so always-on debug events do not pollute model input history.

## Decision
- Emit LLM observability events in normal runtime module execution:
  - Success: `debug`, `llm.raw`
  - Failure: `debug`, `llm.error`, `error`
- Emit concept graph query observability in normal runtime routing:
  - Success: `debug`, `concept_graph.query`
  - Failure: `debug`, `concept_graph.query`, `error`
- Keep payload minimums aligned with the prior tag decision:
  - `llm.raw`: `raw`, `context`, `output_text`, `module`, `mode`
  - `llm.error`: `module`, `mode`, `context`, `error`
  - `concept_graph.query` success: `query_terms`, `limit`, `result_concepts`
  - `concept_graph.query` error: `query_terms`, `limit`, `error`

## Why
- Operational debugging requires parity between debug and normal runs.
- Failure visibility must be guaranteed; relying on success-oriented raw events is insufficient.
- Always-on debug events preserve diagnostics while keeping LLM context clean through existing debug-event filtering.

## Scope
- Implemented in `core-rust/src/application/pipeline_service.rs`.
- No interface contract changes; this is observability behavior expansion only.
