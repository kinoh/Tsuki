# Router Responsibility Shift to Integrated Routing Execution

## Overview
This change moves routing policy and routing-stage execution responsibilities into the router path in `core-rust`.

## Problem
The previous flow split routing concerns across two places:
- Router emitted only query terms.
- Application later performed concept-graph query, trigger selection, and hard-trigger execution preparation.

This made ownership ambiguous and allowed drift between the intended router role and runtime behavior.

## Decision
- Expanded router runtime output to include:
  - `activation_query_terms`
  - `concepts`
  - `hard_triggers`
  - `soft_recommendations`
  - `hard_trigger_results`
- Updated `run_router` to perform, in order:
  1. LLM-based query-term inference (with deterministic lexical fallback)
  2. concept-graph `concept_search`
  3. hard/soft trigger selection
  4. hard-trigger submodule execution
- Updated decision runtime to consume router-produced activation and hard-trigger execution results without re-running activation/trigger logic.

## Why
- Aligns runtime behavior with the clarified architecture where router owns routing decisions and activation-stage operations.
- Removes duplicate policy logic from decision/application stage.
- Keeps decision stage focused on consuming prepared context and deciding subsequent tool usage.

## Implementation Notes
- Router LLM call is tool-free (`tools = []`) and used only to infer activation query terms.
- If router LLM inference fails or returns unusable output, lexical fallback query terms are used.
- Existing concept-graph observability (`debug,concept_graph.query`) remains intact.
- Router LLM observability now uses existing debug LLM event emission with `module=router` source ownership.

## Scope
- Implemented in `core-rust/src/application/pipeline_service.rs`.
- No external API contract changes.
- No backward compatibility constraints applied (`core-rust` is explicitly non-deployed/WIP).
