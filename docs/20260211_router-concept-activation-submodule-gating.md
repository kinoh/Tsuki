# Router-First Concept Activation and Submodule Gating

## Context
- Current runtime executes all active submodules for each user input in the normal flow.
- This is resource-heavy and does not match the intended architecture.
- The target is router-first orchestration with concept-graph-driven activation.

## Final Decisions

### 1) Router Output Schema
- Keep output schema minimal and sufficient.
- Required fields:
  - `concept_activation` (current activated concepts and scores)
  - `soft_recommendations` (recommended submodules)

### 2) Threshold Source
- Threshold values are managed in configuration files.

### 3) Decision Control Semantics
- `reflect` is not used.
- Submodules are always provided to Decision as tools.
- Soft trigger is recommendation only; it is not a forced execution signal.

### 4) Re-run Stop Condition
- No dedicated stop-condition policy is introduced in this design.

### 5) Decision Input
- Decision input contains:
  - concept state and activation
  - submodule recommendations
- Top-N count follows MCP-side conventions.

### 6) `submodule_outputs` Override Rule
- If a matching submodule event exists in history, override that value.
- If no matching event exists, insert at the latest position in the input event sequence.

### 7) Event Design
- No activation snapshot event is introduced.

### 8) Failure Policy
- No special failure policy is introduced.

### 9) Observability / KPI
- No dedicated KPI requirement is introduced for this phase.

### 10) Rollout Strategy
- No dedicated rollout strategy is introduced for this phase.

## Concept-Graph Integration Boundary
- Concept-graph access is application-led by default.
- Router / activation / decision-prep paths use in-process library access.
- MCP exposure is optional for secondary use cases only.

## Explicit Notes from User Feedback
- Executing all submodules on every input is overuse of resources.
- Human-like behavior should not assume constant deep deliberation.
- Decision-side recommendation should remain recommendation, not forced control.
- `reflect` was removed due to semantic confusion in this context.
