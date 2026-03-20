# Decision: Debug Observability Event Tags for Core Runtime

## Context
- The runtime already emits primary module output/error events and debug `llm.raw` events in debug-run paths.
- We need minimum observability for:
  - concept-graph query behavior,
  - module input/output visibility,
  - OpenAI API failures.
- Debug events must be excluded from module history input.

## Confirmed Constraints
- Keep existing `error` tag semantics; do not introduce extra status tags like `phase:error`.
- Use meaningful composite tags (for example `concept_graph.query`) instead of low-information tags like `query`.
- For concept-graph query observability, emit only `success` and `error` outcomes.
- Do not emit `start` events for concept-graph query in this system, because operational uncertainty is low.
- Include query arguments directly in success/error payloads.

## Decision
- Add concept-graph query debug events with the following tags:
  - success: `debug`, `concept_graph.query`
  - failure: `debug`, `concept_graph.query`, `error`
- Add explicit LLM failure debug events:
  - failure: `debug`, `llm.error`, `error`
- Keep using existing `debug`, `llm.raw` for successful raw-response observability.

## Payload Minimums
- `concept_graph.query` success:
  - `query_terms`
  - `limit`
  - `result_concepts`
- `concept_graph.query` error:
  - `query_terms`
  - `limit`
  - `error`
- `llm.error`:
  - `module`
  - `mode` (when available)
  - `context` (or summary if truncation policy is introduced)
  - `error`

## Rationale
- Reusing `error` keeps filtering and dashboards consistent with existing event semantics.
- Composite tags improve discoverability and reduce ambiguous tag filtering.
- Emitting only terminal outcomes (`success`/`error`) avoids noise while preserving replay-grade diagnostics.
- Explicit `llm.error` is necessary because `llm.raw` is success-oriented and does not guarantee equivalent failure visibility.

## Existing Behavior (for alignment)
- Debug events are already excluded from history assembly by `is_debug_event`.
- Existing non-debug module output/error events remain the primary runtime semantic stream.

## Notes From User Feedback
- Avoid redundant status dimensions when `error` tag already exists.
- Prefer domain-specific tag names such as `concept_graph.query`.
- Prioritize actionable signal over exhaustive lifecycle tracing for low-uncertainty internal operations.
