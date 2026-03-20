# Refactor Phase 1: Extract Improve Service from `main.rs`

## Context
- `core-rust/src/main.rs` had mixed transport handlers and self-improvement domain logic.
- The agreed refactor plan prioritizes extracting self-improvement flow first because it has high cohesion and clear boundaries.

## Decision
- Added `core-rust/src/application/improve_service.rs`.
- Added `core-rust/src/application/mod.rs`.
- Moved self-improvement core logic out of `main.rs`:
  - trigger/proposal/review processing
  - auto-approval check for `Memory` section
  - projection application and projection error event emission
  - prompt target parsing and markdown section replacement helpers
- Kept `debug_improve_*` handlers in `main.rs` as transport entrypoints only.
  - They now delegate to `improve_service::{trigger_improvement, propose_improvement, review_improvement}`.

## Why
- This keeps `debug` naming and HTTP concerns at transport level only.
- Improves cohesion by grouping proposal/review/projection rules into one service.
- Reduces accidental coupling in `main.rs` while preserving existing API behavior.

## Interface Notes
- `AppState`, `Modules`, and `ModuleRuntime` were made `pub(crate)` with `pub(crate)` fields needed by service logic.
- `DebugImprove*Request` fields were made `pub(crate)` to allow service-level access.
- `record_event` was made `pub(crate)` to centralize event append/broadcast/log behavior.

## Behavior Guarantee
- Route paths and payload shapes are unchanged.
- Event tags and payload semantics are unchanged.
- Build check: `cargo check` passed after extraction.
