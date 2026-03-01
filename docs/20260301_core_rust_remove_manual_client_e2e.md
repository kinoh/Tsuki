# Core-Rust Removal of Manual Client E2E Assets

## Overview
This change removes the manual client-side E2E assets from `core-rust` and keeps integration testing centered on the LLM-driven integration harness.

Compatibility Impact: breaking-by-default (manual scenario client assets removed, no fallback path retained).

## Problem Statement
`core-rust` had two parallel E2E paths:
- manual fixed-turn WebSocket scenarios (`tests/client` + `examples/test_runner.rs`/`ws_scenario.rs`/`format_log.rs`)
- integration harness scenarios (`tests/integration` + `examples/integration_harness.rs`)

The manual path was no longer aligned with current validation goals:
- fixed-turn scripts are not realistic for current conversational behavior validation
- JSONL capture-only flow did not provide pass/fail gates tied to explicit quality metrics
- maintaining two scenario stacks increased maintenance overhead and ambiguity about the canonical test path

## Decision
Adopt a single E2E test path in `core-rust`: integration-harness-based scenarios under `tests/integration`.

Removed assets:
- `core-rust/tests/client/`
- `core-rust/examples/test_runner.rs`
- `core-rust/examples/ws_scenario.rs`
- `core-rust/examples/format_log.rs`

Updated guidance:
- `core-rust/README.md` now removes manual scenario instructions.
- `core-rust/tests/README.md` now points only to integration harness workflow.

## Why
- Keeps test responsibility explicit: quality evaluation belongs to scenario+judge metrics, not ad-hoc log reading.
- Reduces duplicated flows and lowers maintenance cost.
- Aligns repository operation around one clear source of truth for E2E behavior checks.

## Notes
- Historical design docs that describe manual E2E tooling are kept as historical records.
- Current operational/testing entrypoints are defined in:
  - `core-rust/Taskfile.yaml`
  - `core-rust/tests/integration/README.md`
