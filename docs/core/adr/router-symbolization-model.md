---
date: 2026-03-15
---

# ADR: Router Symbolization — Literal Description, LLM Seed Filter Removed

## Context

The router had an embedded LLM call (`run_router_llm`) that selected concept recall seeds from
embedding candidates. This added latency with no clear quality advantage: embedding-based retrieval
already performs semantic filtering, making a second LLM filter redundant.

Multimodal inputs (images, audio) were also not symbolized before embedding — raw text was passed
to concept search, making images invisible to the router.

## Decision

- Router LLM call for seed selection is removed entirely.
- All embedding candidates from vector search are passed directly as activation seeds.
- Symbolization (text description of multimodal input) is introduced as a pre-retrieval step.

### Symbolization contract

- Text-only input: returned as-is, no LLM call.
- Input with media: calls `RouterSymbolizer` (OpenAI Responses API with vision).
- System instruction: `"Describe the provided input literally and concisely in plain text."`
- Symbolization is **literal**, not impressionistic prose.

The concept graph activation state is tsuki's subjective impression — expressed as active concepts
and arousal values. Symbolization must not replicate that role.

### Signal-to-noise ratio over precision

Noise in concept activation can serve as creative seeds for novel associations. The goal is
sufficient S/N ratio, not exact recall. The LLM seed-selection filter was optimizing for precision
at the cost of latency and creativity.

## Rationale

Embedding-based retrieval already provides semantic filtering. A second LLM filter duplicates work
and adds hot-path latency without measurable quality benefit. Literal symbolization makes
multimodal content visible to vector search without injecting interpretive bias.

## Compatibility Impact

breaking-by-default (no compatibility layer)
