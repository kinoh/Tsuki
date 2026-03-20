# Router Embedding Feasibility Probe (SSE Japanese Quantized)

## Overview
This document records a feasibility probe for replacing primitive lexical concept lookup in `core-rust` router preprocessing with semantic vector retrieval.

Target model:
- Hugging Face: `RikkaBotan/quantized-stable-static-embedding-fast-retrieval-mrl-ja`

Date:
- March 1, 2026

## Problem Statement
Router preprocessing currently derives query terms from character-class segmentation and performs concept lookup mainly via lexical substring matching (`CONTAINS`) plus arousal fallback.

This is fast but brittle for paraphrases and implicit semantic matches.

We needed to validate whether the target Japanese embedding model can be executed from Rust and produce stable vectors suitable for concept retrieval.

## Solution
A standalone probe binary was added:
- `core-rust/src/bin/verify_sse_embedding.rs`

It performs the full local inference path:
1. Load `tokenizer.json` via `tokenizers` crate.
2. Load DyT parameters (`dyt.alpha`, `dyt.beta`, `dyt.bias`) from `model_rest.safetensors`.
3. Load `embedding.q4_k_m.bin` and dequantize rows on-the-fly (Q4 packed + per-row scale).
4. Compute `EmbeddingBag(mean)` over token ids.
5. Apply DyT transform: `beta * tanh(alpha * x + bias)`.
6. L2-normalize embedding and compute cosine similarity for retrieval ranking.

The probe also prints a lexical baseline ranking (router-style query term extraction approximation) to visualize contrast.

## Design Decisions
- Keep this as a probe binary first, not production router integration.
Reason: feasibility had to be confirmed before introducing schema/index/runtime changes.

- Reconstruct model math directly in Rust rather than adding Python runtime.
Reason: production path should stay native to `core-rust` runtime and deployment model.

- Prefer minimal dependency addition (`tokenizers`, `safetensors`) and explicit file loading.
Reason: keeps integration surface clear and auditable.

## Validation Result
Probe run succeeded with the real model artifacts and produced embeddings:
- `MODEL_OK hidden_dim=512 vocab_size=32768`

Observed behavior in built-in Japanese demo queries:
- Semantic ranking returned contextually relevant concepts (e.g., `星を見に行きたい` -> `星空`, `天体観測`).
- Lexical baseline remained sparse/fragile when direct form overlap was limited.

Conclusion:
- Vector embedding with this model is technically feasible in `core-rust`.
- Result quality indicates clear potential for improving router concept candidate discovery over substring-only search.

## Compatibility Impact
- Breaking-by-default policy unaffected.
- No API/event contract change in this step.
- This is an additive probe-only change.

## Future Considerations
- Add a persistent concept embedding index and nearest-neighbor retrieval path.
- Define re-embedding policy for concept node updates.
- Add numeric acceptance criteria before production switch (latency target, retrieval quality target, fallback policy).
- Validate quality using real concept graph snapshots and router scenarios, not only synthetic demo candidates.
