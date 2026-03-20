# Pipeline Service Refactor to Orchestration-Only

## Overview
This refactor aligns file boundaries with responsibility boundaries by reducing `pipeline_service` to orchestration-only behavior.

## Problem
`pipeline_service` had accumulated non-orchestration responsibilities:
- debug-run flow details,
- decision/submodule execution internals,
- history formatting and debug override logic.

This made boundary ownership unclear and complicated reviews.

## Decision
- Keep `pipeline_service` as orchestration entrypoints only:
  - `handle_input`
  - `run_debug_module` delegation
- Extracted responsibilities into dedicated application modules:
  - `application/router_service.rs`
    - router output types
    - query-term inference (LLM + fallback)
    - concept-graph activation query
    - hard/soft trigger selection
    - hard-trigger execution orchestration
    - router-side observability event emission for routing/query stages
  - `application/debug_service.rs`
    - debug request flow
    - debug input append policy
    - input parsing + event append helper
  - `application/execution_service.rs`
    - decision execution
    - submodule execution
    - debug module execution internals
    - prompt/module loading helpers
  - `application/history_service.rs`
    - event history retrieval/formatting
    - decision debug submodule-output override logic

## Why
- Enforces architectural boundaries at file level.
- Makes orchestration logic easy to inspect without mixed execution details.
- Isolates debug and formatting policy from runtime orchestration.

## Scope
- Updated `core-rust/src/application/mod.rs` exports.
- Replaced `core-rust/src/application/pipeline_service.rs` with thin orchestration wrappers.
- No external HTTP/WS API changes.

## Consolidation
- This document supersedes:
  - `docs/20260215_router-service-file-boundary.md`
