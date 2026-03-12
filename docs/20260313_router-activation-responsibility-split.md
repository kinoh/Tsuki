# Router Activation Responsibility Split

## Overview
This document records the responsibility split for the router-side multimodal activation flow.
The goal is to keep the router focused on activation and event emission without leaking into downstream decision responsibilities.

Compatibility Impact: No contract change is required by this document itself. It defines the intended file-level ownership for future refactors.

## Problem Statement
The multimodal activation work introduced several concerns into the router path:
- external input interpretation
- auxiliary verbalization
- embedding-based concept retrieval
- concept activation state updates

Without an explicit split, `router_service.rs` risks becoming a mixed boundary that owns vendor calls, graph policy, and downstream orchestration.

## Solution
The router boundary should own orchestration for activation only.
It should not own downstream decision logic and should not introduce a separate ingress service for what is still router-local input interpretation.

## Responsibility Assignment
### `core-rust/src/application/router_service.rs`
- Own router-local input interpretation from `RouterInput`
- Call auxiliary verbalization
- Call concept retrieval
- Call concept activation
- Emit router state and debug events

Must not own:
- decision planning
- respond/ignore choice
- execution/module selection after router activation

### `core-rust/src/input_ingress.rs`
- Own shared router input types
- Define media attachment payload shapes

Must not own:
- router behavior
- LLM calls
- graph access

### `core-rust/src/application/auxiliary_verbalization_service.rs`
- Own creation of auxiliary verbalization from `RouterInput`
- Convert image, audio, and future sensory-text inputs into a common auxiliary verbalization shape

Must not own:
- vector search
- graph activation
- router event emission

### `core-rust/src/auxiliary_verbalizer.rs`
- Own the vendor adapter for OpenAI-based auxiliary verbalization
- Translate service requests into API calls and responses back into typed outputs

Must not own:
- router orchestration
- graph policy

### `core-rust/src/application/concept_retrieval_service.rs`
- Own Gemini embedding request construction for router retrieval
- Own concept graph vector search invocation
- Return scored concept candidates

Must not own:
- activation state mutation
- router event emission

### `core-rust/src/application/concept_activation_service.rs`
- Own conversion from scored concept candidates to active concept state
- Own recall/arousal update policy
- Return the active concept snapshot used by the router event

Must not own:
- raw input interpretation
- auxiliary verbalization generation
- downstream decision behavior

### `core-rust/src/activation_concept_graph.rs`
- Own persistence primitives
- Own vector index management
- Own concept, relation, episode, and activation-state read/write operations

Must not own:
- router stage ordering
- OpenAI/Gemini application flow decisions
- downstream decision logic

## Rejected Splits
### Separate `InputIngressTranslator` service
Rejected because router input interpretation is still part of the router boundary.
Creating a dedicated ingress service would add an artificial layer for logic that is not reused outside router activation.

### Pulling `PipelineService` or downstream decision services into this split
Rejected because they are outside the router activation boundary.
This design is intentionally scoped to the router path ending at router event emission.

## Target Flow
- raw payload
- `RouterInput`
- `AuxiliaryVerbalization`
- Gemini query vector
- scored concept candidates
- active concept snapshot
- router activation/debug events

## Future Considerations
- If auxiliary verbalization starts serving non-router consumers, it may justify its own domain interface package.
- If concept activation policy grows substantially, the activation service should expose its own typed policy inputs rather than passing graph-oriented primitives through router code.
