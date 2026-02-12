# Router-First Concept Activation and Submodule Gating

## Background
Current runtime behavior executes all active submodules for each user input in the normal flow. This increases cost and latency, and it does not match the intended interaction style where deeper processing should happen conditionally.

Decision currently consumes history-oriented text, while internal activation intent is not represented as a dedicated state. This makes orchestration harder to control and reason about.

## Goal
Move orchestration to a router-first model where concept-graph activation is the primary internal state, and submodule execution is gated by router output instead of always-on fan-out.

## Scope
This document defines runtime orchestration for:
- router output and activation-driven submodule recommendation,
- decision input composition,
- `submodule_outputs` override behavior in composed input history,
- integration boundary between application runtime and concept-graph access.

Out of scope:
- dedicated KPI framework for this phase,
- dedicated rollout plan for this phase,
- special-case failure handling beyond existing behavior.

## System Model
### Components
- Router:
  - reads new user input,
  - reads concept graph state,
  - emits minimal activation output.
- Application orchestrator:
  - receives router output,
  - decides submodule calls,
  - composes decision input,
  - invokes decision.
- Decision:
  - receives context and tool availability,
  - returns normal decision output.
- Submodules:
  - exposed to Decision as tools at all times.

### Router Output (minimal schema)
- `concept_activation`:
  - active concepts and current activation scores.
- `soft_recommendations`:
  - recommended submodule names.

## Runtime Flow
1. User input arrives.
2. Router computes activation and returns minimal output.
3. Application composes decision input from:
   - concept state and activation,
   - submodule recommendations,
   - existing event history context.
4. Decision runs with submodules available as tools.
5. If Decision chooses to use a submodule tool, normal event flow records that execution and output.

## Decision Semantics
- Soft trigger output from router is recommendation only.
- Submodule execution is not forced by soft recommendation; Decision can choose whether to call tools.

## Threshold Policy
Threshold values are managed in configuration files.

## Decision Input Contract
Decision input contains:
- concept state and activation,
- submodule recommendations,
- production-style event-history context.

Top-N count for concept activation follows the original core conventions.

## `submodule_outputs` Override Rule
When composing history for decision input:
- if a matching submodule event exists, override its value,
- if no matching event exists, insert the provided output at the latest position in the input event sequence.

This rule applies to composed input context; it does not require additional synthetic persistence events.

## Integration Boundary
Concept-graph access is application-led by default:
- router / activation / decision-prep paths use in-process library access.
- No MCP exposure.

Core activation paths should not depend on MCP round-trip latency.

## Notes from Discussion
- Running all submodules for every input is considered resource overuse.
- Human-like response behavior should not assume constant deep deliberation.
- Recommendation must remain recommendation; avoid hidden forced control semantics.
