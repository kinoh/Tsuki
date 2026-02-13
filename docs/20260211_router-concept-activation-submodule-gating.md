# Router-First Concept Activation and Submodule Gating

## Background
Current runtime behavior executes all active submodules for each user input in the normal flow. This increases cost and latency, and it does not match the intended interaction style where deeper processing should happen conditionally.

Decision currently consumes history-oriented text, while internal activation intent is not represented as a dedicated state. This makes orchestration harder to control and reason about.

## Goal
Move orchestration to a router-first model where concept-graph activation is the primary internal state, and submodule execution is gated by router output instead of always-on fan-out.

## Scope
This document defines runtime orchestration for:
- router output and activation-driven submodule recommendation,
- router-driven hard trigger execution at application orchestration stage,
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
- `hard_triggers`:
  - submodule names that must be executed by the application before decision.
- `soft_recommendations`:
  - recommended submodule names.

## Runtime Flow
1. User input arrives.
2. Router computes activation and returns minimal output.
3. Application executes `hard_triggers`.
4. Application composes decision input from:
   - concept state and activation,
   - hard-trigger execution results,
   - submodule recommendations,
   - existing event history context.
5. Decision runs with submodules available as tools.
6. If Decision chooses to use a submodule tool, normal event flow records that execution and output.

## Decision Semantics
- Hard trigger is force execution at application orchestration stage.
- Soft trigger output from router is recommendation only.
- Submodule execution is not forced by soft recommendation; Decision can choose whether to call tools.

## Threshold Policy
Threshold values are managed in configuration files.

## Decision Input Contract
Decision input contains:
- concept state and activation,
- hard trigger execution results,
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
- Even with in-process access, the concept-graph module must provide an interface equivalent to
  `mcp/concept-graph` so behavior and integration contracts stay aligned.

Core activation paths should not depend on MCP round-trip latency.

## Notes from Discussion
- Running all submodules for every input is considered resource overuse.
- Human-like response behavior should not assume constant deep deliberation.
- Recommendation must remain recommendation; avoid hidden forced control semantics.
- Hard trigger execution is handled by the application after router output, not by Decision recommendation semantics.

## Follow-up Clarification (2026-02-13)
- Router is clarified as a language-ambiguity absorber that outputs query terms; it should not execute tools in activation path.
- Application activation path reads concept-graph state in-process and does not depend on MCP transport round-trip.
- Submodule-purpose-driven graph mutation is handled by submodules, not by application-level embedded keyword heuristics.
- Consolidated responsibility and interface details are documented in:
  - `docs/20260213_router-concept-graph-interface-and-responsibility-clarification.md`
