# Router Preconscious Concept-Graph Tool Design

## Context
- In `core-rust`, router is the preconscious module responsible for associative filtering and information selection.
- Router latency is currently dominated by multi-round LLM + tool-call loops.
- For integration and runtime stability, router must run in a one-LLM-call path.

## Decision
- Router runtime path is one LLM call only.
- Router must not run LLM tool-call loops.
- `concept_search` is executed in application-side preprocessing (non-LLM) before the router LLM call.
- `recall_query` is not used in router runtime path.
- Router receives pre-fetched concept candidates as context and performs ambiguity resolution + final state shaping in one pass.

## Data Model
- No taxonomy split between "activated concepts" and "recalled facts".
- No distinction between concept and episode at runtime selection level.
- Router output exposes a single text state:
  - `active_concepts_from_concept_graph: String`
- `concept_search` candidate list is intermediate preprocessing input and must not be emitted as router module output.

## Scope Constraints
- No execution policy specification is required in code for this design document.
- No additional round-limit config such as `max_router_tool_rounds` or `max_module_recall_rounds`.
- No migration plan is required.
- No backward compatibility requirement applies (`core-rust` is under development and not deployed).

## Query-Term Extraction (v0)
- Keep extraction simple and deterministic.
- Perform text normalization first (whitespace/punctuation/full-width normalization).
- Build query terms by character-class segmentation:
  - split by transitions across Japanese scripts / alnum classes (kanji, hiragana, katakana, latin/digit, others).
- Build additional position-variant terms from segmented text:
  - full segment
  - prefix slice
  - suffix slice
  - middle slice
- De-duplicate terms and drop too-short/noisy tokens.
- Cap final term list by `router.query_terms_max`.
- No morphological analysis, no proper-noun model, no manual synonym dictionary in v0.

## Implementation Direction
- Add application-side preprocessing that:
  - extracts query terms with v0 rules
  - executes `concept_search` once (or bounded small fan-out) without LLM tool loops
  - passes candidate concept lines into router context
- Keep router LLM call count at exactly one per input.
- Keep downstream contract unchanged: modules receive `active_concepts_from_concept_graph` as shared context.

## Compatibility Impact
- breaking-by-default (no compatibility layer)
