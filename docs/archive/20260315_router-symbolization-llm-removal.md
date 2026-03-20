# Router Symbolization and LLM Removal

## Overview
This change implements the service extraction plan described in `20260313_router-activation-responsibility-split.md`
and removes the router LLM call, replacing it with symbolization + embedding-based concept retrieval.

The router no longer calls an LLM to select recall seeds from candidate concepts.
Instead, all candidate concepts from vector search are passed directly as activation seeds.

## Problem Statement
The router had an embedded LLM call (`run_router_llm`) that selected concept recall seeds from a candidate list.
This call added latency and introduced an extra model call in the hot path with no clear quality advantage:
embedding-based retrieval already performs semantic filtering, so a second LLM filter is redundant.

Additionally, multimodal inputs (images, audio) were not symbolized before embedding.
Raw input text was passed directly to concept search, meaning images were invisible to the router.

## Solution

### Phase 1 — Extract `concept_activation_service`
Extracted `recall_query` + `active_nodes` orchestration from `resolve_active_concepts_and_arousal` in `router_service.rs`.

New file: `core-rust/src/application/concept_activation_service.rs`

```rust
pub(crate) async fn activate_concepts<G>(
    seeds: &[String],
    active_state_limit: usize,
    graph: &G,
) -> ConceptActivationResult
where
    G: ConceptGraphActivationReader + ConceptGraphOps + ?Sized,
```

- Returns `ConceptActivationResult { active_concepts_and_arousal: String, errors: Vec<String> }`
- Skips `recall_query` when seeds is empty (avoids unnecessary graph call)
- Uses `?Sized` bound to support `&dyn ConceptGraphStore` dispatch through combined trait bounds

### Phase 2 — Extract `concept_retrieval_service`
Extracted embedding and vector search from `preprocess_router_activation` in `router_service.rs`.

New file: `core-rust/src/application/concept_retrieval_service.rs`

```rust
pub(crate) async fn retrieve_concepts(
    query_text: &str,
    input: &RouterInput,
    limit: usize,
    multimodal_config: &RouterMultimodalEmbeddingConfig,
    graph: &dyn ConceptGraphActivationReader,
) -> ConceptRetrievalResult
```

- Accepts `query_text: &str` separately from `&RouterInput` so symbolized text drives text search
  while the raw `RouterInput` drives multimodal embedding
- Returns `ConceptRetrievalResult { candidate_concepts, text_candidate_concepts, multimodal_candidate_concepts, candidate_source, errors }`
- Collects errors rather than panicking; caller emits debug events

### Phase 3 — Implement `router_symbolizer`
New file: `core-rust/src/router_symbolizer.rs`

Defines two traits and one production backend:

```rust
pub(crate) trait RouterSymbolizer: Send + Sync {
    async fn symbolize(&self, input: &RouterInput) -> Result<String, String>;
}
pub(crate) trait SymbolizerBackend: Send + Sync {
    async fn describe(&self, input: &RouterInput) -> Result<String, String>;
}
```

`OpenAIRouterSymbolizer<B>`:
- Text-only inputs: returns text directly, no LLM call
- Inputs with media: delegates to `SymbolizerBackend`

`ResponseApiSymbolizerBackend`:
- Calls the OpenAI Responses API with vision support
- Images: sent as `InputImageContent { image_url: "data:<mime>;base64,<data>" }`
- Audio: sent as a text note `"[N audio clip(s) provided]"` (Responses API has no InputAudio content part)
- System instruction: "Describe the provided input literally and concisely in plain text."
- Imports from `async_openai::types::responses::*` (0.33.0 module layout)

`RouterConfig` gains `symbolizer_model: Option<String>`.
When absent, falls back to `config.llm.model`.

### Phase 4 — Extract `router_symbolization_service`
New file: `core-rust/src/application/router_symbolization_service.rs`

Simple orchestrator that calls `RouterSymbolizer` and falls back to `input.display_text()` on error.

```rust
pub(crate) async fn symbolize(
    input: &RouterInput,
    symbolizer: &dyn RouterSymbolizer,
) -> SymbolizationResult
```

Returns `SymbolizationResult { text: String, error: Option<String> }`.
Errors are surfaced as debug events by the caller, not panicked.

### Phase 5 — Remove router LLM call
`router_service.rs::run_router` rewritten to use the new services:

```
symbolize(input) → retrieve_concepts(symbolized_text, input) → activate_concepts(candidates)
```

Removed:
- `RouterPreprocessOutput`, `RouterConceptResolution` structs
- `preprocess_router_activation`, `resolve_active_concepts_and_arousal`, `render_router_context_template`
- `build_router_config`, `emit_router_debug_raw`, `render_list_for_prompt`
- `router_candidate_source`, `merge_unique_concepts`
- All LLM-related imports: `llm_raw`, `build_response_api_llm`, `LlmRequest`, `LlmUsageContext`, etc.
- `router_context_template` from the old `input` config section (no longer used)

The `_modules` and `_overrides` parameters in `run_router` are now unused (prefixed with `_`).
`parse_recall_seeds` and `render_active_nodes_as_text` are retained with `#[allow(dead_code)]`
because existing tests in `router_service.rs` reference them.

### Wiring
`AppServices` gains `router_symbolizer: Arc<dyn RouterSymbolizer>`.
`server_app.rs` constructs `build_response_api_symbolizer(symbolizer_model)` before `AppState::new`.

## Design Decisions

### Symbolization is literal, not impressionistic
Symbolization converts multimodal input into a dictionary-like literal text description.
It is NOT impressionistic prose.

The concept graph activation state IS tsuki's subjective impression.
This state is expressed as the set of active concepts and their arousal values.
It cannot and should not be expressed as natural language — that is the role of the concept graph.

### S/N ratio over precision
Embedding-based concept retrieval does not need to be perfect.
Noise in concept activation can serve as creative seeds for novel concept associations.
The goal is sufficient S/N ratio, not exact recall.

### All candidates are seeds, no LLM filter
The LLM seed-selection step is removed entirely.
`candidate_concepts` from vector search are passed directly to `activate_concepts` as seeds.
This removes a redundant filtering step and reduces latency.

### `?Sized` for combined trait dispatch
`activate_concepts` uses `G: ConceptGraphActivationReader + ConceptGraphOps + ?Sized`
to allow callers that hold `&dyn ConceptGraphStore` (which is `!Sized`) to pass it through.
Without `?Sized`, the compiler rejects the `impl Trait1 + Trait2` dispatch pattern.

### async-openai 0.33.0 Responses API
Types live under `async_openai::types::responses::*`, not `async_openai::types::*`.
`EasyInputMessage` requires an explicit `r#type: MessageType::Message` field.

## Test Coverage
All new services are covered by unit tests written before implementation (TDD):
- `concept_activation_service`: 6 tests
- `concept_retrieval_service`: 6 tests
- `router_symbolizer`: 4 tests
- `router_symbolization_service`: 2 tests

Total: 60 unit tests pass.

## Files Changed
- `core-rust/src/application/concept_activation_service.rs` (new)
- `core-rust/src/application/concept_retrieval_service.rs` (new)
- `core-rust/src/application/router_symbolization_service.rs` (new)
- `core-rust/src/router_symbolizer.rs` (new)
- `core-rust/src/application/mod.rs` (added new modules)
- `core-rust/src/application/router_service.rs` (LLM removal, new service wiring)
- `core-rust/src/app_state.rs` (added `router_symbolizer`)
- `core-rust/src/config.rs` (added `symbolizer_model`, removed `router_context_template`)
- `core-rust/src/main.rs` (added `mod router_symbolizer`)
- `core-rust/src/server_app.rs` (symbolizer construction and wiring)
