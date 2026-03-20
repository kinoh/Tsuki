# Router Activation Model

## Overview

Router activation is split across focused services. Each service owns one stage of the activation
flow and must not leak into adjacent stages.

## Activation Flow

```
RouterInput
  → router_symbolization_service   (literal text description of input)
  → concept_retrieval_service      (embedding + vector search → candidates)
  → concept_activation_service     (recall_query + active-node snapshot)
  → router event emission
```

## Service Responsibilities

### `application/router_service`

Orchestrates the activation flow. Emits router state and debug events.

Must not own: decision planning, respond/ignore choice, execution/module selection.

### `input_ingress`

Owns shared `RouterInput` types and media attachment payload shapes.

Must not own: router behavior, LLM calls, graph access.

### `application/router_symbolization_service`

Converts `RouterInput` into a literal text description for use in vector search.

Must not own: vector search, graph activation, router event emission.

### `router_symbolizer`

Vendor adapter for OpenAI-based symbolization. Translates service requests into API calls.

Must not own: router orchestration, graph policy.

### `application/concept_retrieval_service`

Generates embedding from symbolized text and queries the concept graph vector index.
Returns scored concept candidates.

Must not own: activation state mutation, router event emission.

### `application/concept_activation_service`

Converts scored candidates into active concept state (recall_query + arousal updates).
Returns the active-node snapshot consumed by the router event.

Must not own: raw input interpretation, symbolization, downstream decision behavior.

### `activation_concept_graph`

Owns persistence primitives, vector index management, and concept/relation/episode read-write.

Must not own: router stage ordering, OpenAI/Gemini flow decisions, decision logic.

## Extension Guidance

- New input modalities belong in `input_ingress` (payload shape) and `router_symbolizer`
  (vendor adapter). The rest of the flow is modality-agnostic.
- If symbolization starts serving non-router consumers, expose it as a typed domain interface
  rather than sharing the router-internal service directly.
- Do not add concept activation to the Decision stage — consume what Router already emitted.
