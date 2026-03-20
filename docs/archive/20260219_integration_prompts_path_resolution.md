# Integration Harness Prompts Path Resolution

## Context
- `core-rust` integration harness creates a temporary runtime directory and writes a patched `config.toml`.
- `core.prompts_file` in `tests/integration/config/runner.toml` was written to `prompts.path` as-is.
- When `prompts_file` was a relative path, it was resolved from the temporary runtime directory, not `core-rust/`.
- As a result, `data/prompts.md` was not loaded during integration runs, and identity-related behavior degraded.

## Decision
- Resolve `core.prompts_file` relative to `CARGO_MANIFEST_DIR` inside the integration harness.
- Persist the resolved absolute path into the temporary `config.toml` under `[prompts].path`.
- Fail fast when the resolved file does not exist.
- Enable the integration default by setting:
  - `tests/integration/config/runner.toml`
  - `[core].prompts_file = "data/prompts.md"`

## Why
- Integration runs should be independent from the current working directory.
- Prompt loading must be deterministic across local runs and task-based execution.
- Failing early on missing prompt files avoids silent misconfiguration and misleading evaluation scores.
