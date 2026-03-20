# core-rust `main.rs` Responsibility Refactor Plan

## Context
- `core-rust/src/main.rs` has grown to a large multi-responsibility file.
- Current code mixes transport handlers, runtime pipeline orchestration, self-improvement projection logic, and formatting/parsing utilities.
- The goal is to improve change safety, readability, and testability without changing runtime behavior.

## Current Responsibility Clusters
- Bootstrap and dependency wiring:
  - startup, state construction, route registration.
- WebSocket transport:
  - auth handshake and event streaming.
- Debug HTTP endpoints:
  - prompts, module run, events, improve trigger/proposal/review.
- Self-improvement application:
  - proposal/review semantics, projection, error emission.
- Runtime pipeline orchestration:
  - input handling, submodule execution, decision execution.
- Event history and parsing utilities:
  - history formatting, role mapping, truncation, decision field parsing.

## Main Problems
- Low cohesion:
  - HTTP handlers directly contain domain logic.
- Weak boundaries:
  - string conventions (`improve.*`, payload keys) are duplicated across call sites.
- High coupling:
  - broad `AppState` access from many functions increases accidental break risk.
- Hard reviewability:
  - behavior changes and structural edits are hard to separate in one file.

## Refactor Direction
- Keep behavior unchanged; move code by responsibility.
- Make handlers thin:
  - validation + service call + HTTP mapping only.
- Consolidate self-improvement semantics into one service boundary.
- Isolate formatting/parsing helpers from orchestration logic.

## Target Module Layout
- `core-rust/src/bootstrap.rs`
  - app startup, state wiring, router construction.
- `core-rust/src/transport/ws.rs`
  - websocket auth and stream handling.
- `core-rust/src/transport/debug_handlers.rs`
  - debug HTTP endpoint entrypoints only.
- `core-rust/src/application/improve_service.rs`
  - improve trigger/proposal/review flow and prompt projection.
- `core-rust/src/application/pipeline_service.rs`
  - input flow, submodule/decision execution, debug run orchestration.
- `core-rust/src/domain/history.rs`
  - event history formatting, role mapping, truncation, decision parsing helpers.

## Separation Rules
- Handlers must not contain core business decisions.
- Service layer owns:
  - prompt mutation
  - event emission policy
  - improve approval/projection logic
- Domain helper layer owns pure functions and formatting/parsing.
- Shared payload/tag keys for improve events should be centralized as constants.

## Phased Plan
1. Extract self-improvement flow first.
   - Move improve handlers and projection helpers into `application/improve_service.rs`.
   - Keep endpoint signatures stable.
2. Extract runtime pipeline flow.
   - Move debug run + runtime run orchestration into `application/pipeline_service.rs`.
3. Extract websocket transport.
   - Move ws auth/stream functions into `transport/ws.rs`.
4. Extract pure utility/domain functions.
   - Move history/role/parse helpers into `domain/history.rs`.
5. Final pass.
   - Minimize `main.rs` to bootstrap/router only.

## Acceptance Criteria
- No externally observable API changes:
  - same routes
  - same payload shapes
  - same event tags and payload semantics
- `cargo check` passes after each phase.
- Existing debug UI behavior remains unchanged.
- Structural changes and behavior changes are committed separately when possible.

## Non-Goals
- No prompt policy redesign in this refactor.
- No scheduler introduction.
- No event schema redesign.

## Risks and Mitigations
- Risk: behavior drift during move.
  - Mitigation: phase-by-phase extraction and compile checks at each step.
- Risk: accidental circular dependencies between service modules.
  - Mitigation: keep service interfaces narrow and pass explicit dependencies.
- Risk: hidden coupling via helper functions.
  - Mitigation: move pure utilities last and keep function contracts stable.
