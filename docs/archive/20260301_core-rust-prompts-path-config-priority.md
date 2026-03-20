# Core Rust Prompts Path Config Priority

## Overview
Integration runs failed because `core-rust` always loaded `prompts.md` from `DATA_DIR` and ignored integration-time prompt path overrides written into `config.toml`.

## Problem Statement
The integration harness patches a temporary `config.toml` with `[prompts].path` to make runtime use a specific prompt file.
However, runtime startup path resolution in `src/main.rs` bypassed config and always used:
- `DATA_DIR/prompts.md` (fallback `./data/prompts.md`)

This split responsibility caused startup failure during integration (`prompts file not found`) even when harness prepared a valid prompt path in config.

## Solution
- Add optional prompt settings to runtime config schema:
  - `Config.prompts: Option<PromptsConfig>`
  - `PromptsConfig.path: Option<String>`
- Change startup prompt resolution to:
  1. use `config.toml [prompts].path` when provided (non-empty required)
  2. otherwise keep existing `DATA_DIR/prompts.md` behavior

## Design Decisions
- Prompt source ownership stays in runtime config when explicitly provided.
- Empty `[prompts].path` is rejected immediately (panic) to preserve fail-fast behavior.
- Existing non-integration behavior is preserved by retaining `DATA_DIR` fallback for deployments that do not define `[prompts]`.

## Implementation Details
- `core-rust/src/config.rs`
  - Added `PromptsConfig` and optional `Config.prompts`.
- `core-rust/src/main.rs`
  - Replaced `prompts_path_from_env()` with `prompts_path_from_config(&config)`.
  - New resolver validates non-empty configured path.

## Verification
- `cargo check` passes.
- `task integration/run -- --run-count 1 --scenario tests/integration/scenarios/self_improvement_trigger.yaml` now starts core-rust successfully and completes with `overall_pass=true`.

## Compatibility Impact
non-breaking (config override added; existing `DATA_DIR` path behavior preserved when `[prompts]` is absent)
