# Router Multimodal Embedding Shadow

## Overview
This change adds a multimodal router-ingress path that can carry image and audio payloads to the router while preserving the existing text-driven decision flow.
The router can now query a parallel Gemini multimodal embedding index in Memgraph and compare it with the existing local text embedding search.

Compatibility Impact: Breaking-by-default is preserved for runtime behavior because the new path is disabled unless `router.multimodal_embedding.enabled=true`.

## Problem Statement
The previous router contract accepted only `input_text: &str`.
Even though the WebSocket API spec already mentioned sensory input, the runtime parser reduced every user input to plain text before concept search.
That made it impossible to evaluate whether direct multimodal embeddings produce richer concept activation than caption/transcript-only text embedding.

## Solution
We introduced a dedicated ingress model for router input that can carry:
- user text
- image attachments
- audio attachments

The runtime stores sensory events with explicit media payloads and a generated text summary for history visibility.

The concept graph now supports a parallel multimodal concept index:
- existing local SSE text embedding remains the primary baseline
- Gemini multimodal embeddings are stored separately on `Concept.embedding_multimodal`
- router preprocessing records text candidates, multimodal candidates, and the selected candidate source in the debug event

## Design Decisions
### Keep decision and recall text-first
Only router preprocessing was extended to use multimodal embeddings.
Decision prompting and conversation recall still use text inputs.
Why:
- the validation goal is concept activation quality, not a full multimodal agent rewrite
- this keeps the blast radius narrow and makes A/B comparison possible inside the same runtime

### Use a parallel index instead of replacing the current one
We chose a separate Memgraph vector index for multimodal embeddings.
Why:
- the current local embedding is 512d and already used by concept search plus conversation recall
- replacing it would mix evaluation with a large migration and would hide whether gains come from multimodality or from a different text model

### Persist media in the event contract
We did not stringify media into captions at ingress time.
Why:
- the core hypothesis is that direct multimodal embedding may preserve similarity that captions lose
- media must survive ingress intact if router-stage evaluation is the goal

## Implementation Details
- `input_ingress.rs` defines the minimal router input contract and media attachment schema.
- `debug_service.rs` now parses `images` and `audio` arrays and records `sensory` events.
- `activation_concept_graph.rs` manages both the existing text vector index and the new multimodal concept index.
- `multimodal_embedding.rs` contains the Gemini embedding client and request encoding.
- `router_service.rs` emits debug payloads with:
  - `text_result_concepts`
  - `multimodal_result_concepts`
  - `candidate_source`
  - `result_concepts`

## Future Considerations
- The current evaluation still uses text summaries for downstream decision prompts.
  If multimodal activation proves useful, the next design step should decide whether decision/submodule prompts also need structured sensory context.
- Concept embeddings are still based on concept names only.
  If multimodal gains are weak, the next likely bottleneck is sparse concept-side representation rather than query-side embedding quality.
- Production usage requires `GEMINI_API_KEY` wiring and operator validation before enabling the feature flag.
