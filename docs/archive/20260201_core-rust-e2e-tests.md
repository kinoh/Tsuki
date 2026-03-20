# Core Rust E2E scenario runner

## Context
We wanted a Rust-native equivalent of `core/tests` for the minimal Rust core.
The goal is a simple, scenario-driven WebSocket client plus a runner that can
start `tsuki-core-rust` and record JSONL logs for manual evaluation.

## Decision
- Add Rust examples for:
  - `test_runner` (starts the server unless `--connect` is set, waits for WS readiness).
  - `ws_scenario` (connects to WS, runs YAML scenario, writes JSONL logs).
  - `format_log` (prints a human-friendly view of JSONL logs).
- Store scenarios and docs under `core-rust/tests/` to mirror the TS layout.
- Use YAML for scenarios via `serde_yaml` (dev-dependency) and validate inputs.
- Name the connect-only flag `--connect` to emphasize client-only behavior.

## Rationale
- Keeping the runner and client as examples avoids adding test-only code paths
  to the production binary.
- YAML keeps scenarios readable and consistent with the TS test client.
- A `--connect` flag makes the default E2E workflow explicit while still
  supporting a client-only mode for debugging.

## Notes
- `serde_yaml` is used only for dev tooling and may need revisiting if a
  maintained YAML crate becomes preferable.
