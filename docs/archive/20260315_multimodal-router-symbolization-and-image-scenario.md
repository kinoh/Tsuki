# Multimodal Router: Symbolization Pipeline and Image Integration Scenario

## Summary

This document records the implementation of the symbolization-based router pipeline,
the removal of the router LLM call, and the addition of image-based integration testing.

## Symbolization Pipeline

### Design

Symbolization converts raw multimodal input (images, audio) into literal text for two purposes:

1. **Embedding query** — the symbolized text is the query sent to both the text embedding
   (OpenAI) and the multimodal embedding (Gemini) for concept graph retrieval.
2. **Decision context** — the symbolized text is surfaced in `latest_user_input` so the
   decision LLM understands what sensory content was received.

The concept graph activation state IS tsuki's subjective impression of the input.
Symbolization only produces a literal transcription; it does not interpret or narrate
the impression in natural language.

### Router LLM Removal

Before this branch, the router used an LLM to select recall seeds from candidate concepts.
That LLM call has been removed. The symbolization + embedding retrieval pipeline now
drives concept activation directly: `candidate_concepts` become seeds without an LLM
intermediary.

### New Services

| File | Responsibility |
|------|---------------|
| `application/router_symbolization_service.rs` | Orchestrates symbolizer; falls back to `display_text()` on error |
| `application/concept_retrieval_service.rs` | Runs text + multimodal embedding queries, deduplicates results |
| `application/concept_activation_service.rs` | Recalls and activates concepts from seeds |
| `router_symbolizer.rs` | `RouterSymbolizer` trait + `OpenAIRouterSymbolizer` + `ResponseApiSymbolizerBackend` |

### Sensory Transcription in Decision Context

`latest_user_input` in the decision context is now extended for sensory input:

```
この雰囲気どう思う？

[sensory transcription]
明るいカフェの丸い淡い木目テーブルの上に、紅茶の入った白いカップ…
```

Implementation: `build_decision_input_text()` in `execution_service.rs` compares
`input_text` against `activation_snapshot.symbolized_text`. When they differ (i.e.
media was present), the transcription is appended as a `[sensory transcription]` block.

`symbolized_text` flows: `router_symbolization_service` → `ActivationSnapshot` →
`activation_snapshot_from_router_output` → `execution_service`.

The `concept_graph.query` debug event also records `symbolized_text` for observability.

## Multimodal Embedding (Gemini)

### Configuration

```toml
[router.multimodal_embedding]
enabled = true
shadow_enabled = true
primary_source = "multimodal"
model = "gemini-embedding-2-preview"
output_dimensionality = 512
```

`primary_source = "multimodal"` makes Gemini embedding the primary concept retrieval path.
`shadow_enabled = true` also runs text embedding in parallel for comparison in debug events.

### Startup Probe Skip

`output_dimensionality > 0` skips the startup API probe call that previously hit the
rate limit on free tier keys. The index dimension is taken from config directly.

### Backfill

On first startup with a new memgraph snapshot, `backfill_multimodal_concept_embeddings`
runs to embed all concepts that have `embedding_multimodal IS NULL`. The integration test
backup (`20260315093235744366_timestamp_49547`) has all 377 concepts pre-embedded, so
`MULTIMODAL_CONCEPT_BACKFILL embedded=0 total=0` on every test run.

## Integration Test: Image Atmosphere Scenario

### Location

`tests/integration/scenarios/image_atmosphere.yaml`

### Purpose

Verifies that multimodal embedding produces meaningfully different concept activation
for visually distinct images. Uses two images:

- `assets/western_cafe.jpg` — tea and strawberry cake on a light wood table
- `assets/salmon_dish.jpg` — seared salmon fillet with vegetables on a white plate

### Metrics

| Metric | Pass | Description |
|--------|------|-------------|
| `scenario_requirement_fit` | yes | Response reflects each image's atmosphere |
| `dialog_naturalness` | yes | Natural Japanese conversation |
| `cross_image_differentiation` | yes | ≥4 concepts differ between the two turns |
| `image_concept_relevance` | no (exclude_from_pass) | Concepts match image content; excluded because it depends on graph contents |

### Harness Extension

`ScenarioStep::Sensory` was added to `integration_harness.rs`:

```yaml
- kind: sensory
  images:
    - path: "tests/integration/assets/western_cafe.jpg"
      mime_type: "image/jpeg"
  text: "この雰囲気どう思う？"
```

Images are loaded from disk, base64-encoded, and sent as
`{"type": "sensory", "images": [...], "text": "..."}` over WebSocket.

Binary image data is stripped from events before passing to the judge to avoid
exceeding the context window.

## Observed Results

Turn 1 (cafe/tea): `multimodal_result_concepts` = クリームティー, 紅茶, 高円寺・カフェ巡り, JUNOS CAFE, …
Turn 2 (salmon): `multimodal_result_concepts` = 肉料理, 排骨, きのこ, …

Clearly differentiated. The decision LLM received the sensory transcription and produced
atmosphere-appropriate responses for both images.
