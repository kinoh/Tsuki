---
date: 2026-03-01
---

# Router Embedding Feasibility Probe

## Hypothesis

Semantic vector retrieval can replace primitive lexical concept lookup in the router, improving
paraphrase handling and implicit semantic matches.

## Model Evaluated

`RikkaBotan/quantized-stable-static-embedding-fast-retrieval-mrl-ja` (SSE Japanese quantized)

- Hidden dim: 512, Vocab size: 32768
- Local inference via `tokenizers` + `safetensors` crates; no Python runtime
- Quantization: Q4 packed + per-row scale, dequantized on the fly
- Transform: DyT (`beta * tanh(alpha * x + bias)`) → L2-normalize

## Result

Probe succeeded with real model artifacts.

- Semantic ranking returned contextually relevant concepts for Japanese queries
  (e.g. `星を見に行きたい` → `星空`, `天体観測`)
- Lexical baseline remained sparse when direct form overlap was limited

**Conclusion:** vector embedding with this model is technically feasible in `core-rust` and shows
clear potential for improving concept candidate discovery over substring-only search.

## What This Informed

This probe preceded the introduction of Memgraph vector concept search as the primary router
retrieval path, replacing lexical `CONTAINS` matching.

## Open Questions at Time of Writing

- Re-embedding policy for concept node updates
- Numeric acceptance criteria for production switch (latency, retrieval quality, fallback)
- Quality validation using real concept graph snapshots and router scenarios
