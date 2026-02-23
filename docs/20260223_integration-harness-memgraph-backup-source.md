# Integration Harness Memgraph Backup Source Is Explicit

## Context
Integration runs were using `MEMGRAPH_URI=bolt://localhost:7697` with shared Memgraph state, while only LibSQL runtime DB was isolated per run.
This made submodule-trigger behavior sensitive to whatever concept-graph data happened to be loaded in the test Memgraph instance.

The user requested reproducible runs by explicitly loading a chosen backup file and recording which backup was used.

## Decision
- Keep integration harness interface style aligned with existing runner-config driven design.
- Do not add new CLI parameters for Memgraph selection.
- Require these fields in `tests/integration/config/runner.toml` `[core]`:
  - `memgraph_uri`
  - `memgraph_backup_path`
- Before starting `tsuki-core-rust`, integration harness restores the exact snapshot file from `core.memgraph_backup_path` into `memgraph-test`.
- Integration result JSON includes:
  - `memgraph_uri`
  - `memgraph_backup_path`

## Why
- Removes hidden dependence on ambient/shared Memgraph state.
- Makes each run auditable by recorded snapshot source.
- Preserves existing harness configuration style (runner file as source of truth).
