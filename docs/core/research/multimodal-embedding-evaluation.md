---
date: 2026-03-12
---

# Multimodal Embedding Evaluation (Shadow Path)

## Hypothesis

Direct multimodal embeddings (Gemini) may preserve semantic similarity that text-only captions
lose, leading to richer concept activation for image and audio inputs.

## Setup

- Existing text embedding (SSE Japanese 512d) remains the primary baseline.
- Gemini multimodal embeddings stored separately on `Concept.embedding_multimodal` in Memgraph.
- Router preprocessing queries both indexes in parallel and records both candidate sets.
- Shadow path is disabled unless `router.multimodal_embedding.enabled=true`.
- Decision and conversation recall remain text-first throughout the evaluation.

## Rationale for Parallel Index

Replacing the existing text index would mix evaluation with a large migration and obscure whether
gains come from multimodality or from a different text model. A separate index allows A/B
comparison within the same runtime.

## Media Handling

Media is persisted in the event contract intact (not stringified at ingress). The hypothesis
requires that raw media survive to the embedding step — captions may lose similarity information
that raw embeddings preserve.

## Debug Observability

Router debug events include:
- `text_result_concepts`
- `multimodal_result_concepts`
- `candidate_source`
- `result_concepts`

## Status

Shadow infrastructure implemented. Evaluation ongoing; no conclusion reached at time of archiving.

## Next Steps If Adopted

- Decide whether decision/submodule prompts also need structured sensory context.
- Define promotion path from shadow to primary index.
- Remove parallel-index complexity once a winner is established.
