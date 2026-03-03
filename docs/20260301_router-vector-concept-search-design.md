# Router Vector Concept Search Design (SSE + Memgraph)

## Overview
This document defines the router concept candidate upgrade in `core-rust`.

The previous router preprocessing extracted lexical query terms and used substring-based concept search.
The new design uses sentence embedding from the SSE Japanese quantized model and Memgraph vector search.

Model:
- `RikkaBotan/quantized-stable-static-embedding-fast-retrieval-mrl-ja`

## Problem Statement
Substring matching fails when user utterances are paraphrased or concept names do not appear explicitly.
As a result, router candidate concepts are often sparse or semantically weak before seed selection.

## Scope
- Replace router candidate lookup input from query-term list to full utterance text.
- Remove `extract_query_terms_v0` from router runtime path.
- Remove runtime mode toggle (`router.concept_search_mode` is not introduced).
- Use Memgraph vector index + `vector_search.search` as the single candidate path.

## Architecture

### Responsibility
- Router (`application/router_service.rs`):
  - passes full latest user input to concept graph search
  - consumes ranked concept candidates
  - keeps seed selection and recall query behavior
- Concept graph store (`activation_concept_graph.rs`):
  - owns embedding model loading
  - owns vector index validation/creation
  - owns vector search and semantic/arousal hybrid ranking
  - owns embedding updates on concept mutations

### Data Contract
- `ConceptGraphActivationReader::concept_search` changed:
  - before: `concept_search(keywords: &[String], limit: usize)`
  - after: `concept_search(input_text: &str, limit: usize)`

### Router Context
- Router preprocessing section now carries only:
  - `candidate_concepts`
- `query_terms` context is removed.

## Embedding and Vector Search

### Embedding Runtime
The store loads model artifacts at startup:
- `tokenizer.json`
- `model_rest.safetensors`
- `embedding.q4_k_m.bin`

Embedding pipeline:
1. tokenize input text
2. dequantize token rows (Q4 packed + per-row scale)
3. mean pooling (EmbeddingBag-like)
4. apply DyT transform (`beta * tanh(alpha * x + bias)`)
5. L2 normalize

### Memgraph Index
Store ensures vector index exists on startup.
If missing, it creates:
- index name: `concept_embedding_idx` (default)
- label/property: `:Concept(embedding)`
- metric: `cos`
- dimension: model hidden size (512 for this model)

Dimension mismatch is treated as startup error.

### Candidate Ranking
1. `vector_search.search(index, k_raw, query_embedding)`
2. collect `(concept_name, similarity)`
3. combine with concept arousal:
   - `final = semantic_weight * similarity + arousal_weight * arousal`
4. return top `limit`

Default weights:
- semantic: `0.75`
- arousal: `0.25`

## Embedding Lifecycle

### Incremental Update
Concept embedding is upserted when concepts are touched by:
- `concept_upsert`
- `episode_add` (linked concepts)
- `relation_add` (concept endpoints)
- concept creation path in `update_affect`

### Initial Bootstrap / Model Change
A one-time full concept backfill is required:
- reason: old concepts may have no embedding
- reason: mixed vectors from different model versions are not guaranteed to share a compatible space

This decision was explicitly re-confirmed with user feedback in this task:
- full re-embedding is required at initialization and model updates

Operational entrypoint:
- `tsuki-core-rust backfill --limit <N>`

## Failure Policy
- Fail fast if model artifacts are missing or invalid.
- Fail fast on incompatible vector index dimension.
- No lexical search fallback path is introduced.

## Observability Changes
`concept_graph.query` event payload now includes:
- `query_text`
- `limit`
- `result_concepts`
- `selected_seeds`
- `active_concepts_from_concept_graph`

Admin concept graph timeline UI now renders `query_text` instead of `query_terms`.

## Delivery/Runtime Impact
- Runtime env wiring updated in `compose.yaml`:
  - `CONCEPT_EMBEDDING_MODEL_DIR=/opt/tsuki/models/quantized-stable-static-embedding-fast-retrieval-mrl-ja`
- Operator notes updated in `core-rust/README.md`.

## Compatibility Impact
- Breaking-by-default accepted.
- Internal contract changed for router-to-store concept search input.
- No external API compatibility promise is maintained for debug payload fields.
