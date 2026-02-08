# Refactor Phase 2: Extract Pipeline Service from `main.rs`

## Context
- After phase 1, improve-domain logic was extracted, but `main.rs` still contained runtime pipeline orchestration and debug-run internals.
- The agreed plan called for moving runtime/debug execution flow into application-level service modules.

## Decision
- Added `core-rust/src/application/pipeline_service.rs`.
- Added module export in `core-rust/src/application/mod.rs`.
- Moved these responsibilities out of `main.rs`:
  - debug module run orchestration (`decision`, `submodule`, `submodules`)
  - websocket input handling pipeline
  - debug input append policy and related history checks
  - runtime submodule/decision execution
  - debug worklog/raw event emission for module runs
  - event history formatting, role mapping, truncation for prompt history
  - decision output parsing helpers
- Kept transport entrypoints in `main.rs` and made them thin:
  - `debug_run_module` delegates to `pipeline_service::run_debug_module`
  - websocket text handling delegates to `pipeline_service::handle_input`

## Why
- Keeps transport concerns (`axum` handlers and websocket routing) in `main.rs`.
- Groups execution logic with a single cohesive boundary (`pipeline_service`).
- Reduces coupling and file size of `main.rs`, making further extraction safer.

## Interface Notes
- `DebugRunRequest` fields and `DebugRunResponse` payload field were made `pub(crate)` for service-level use.
- `record_event` remains in `main.rs` as shared append/broadcast/log primitive and is used by services.

## Outcome
- `main.rs` reduced from ~1300 lines to ~550 lines in this phase.
- Runtime behavior and API routes remain unchanged.
- Build check: `cargo check` passed after extraction.
