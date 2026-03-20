# LLM Integration Tests

This directory contains the rebuilt integration test assets for `core-rust`.

## Layout
- `config/`: runner-level configuration (`tester` and `judge` model/prompt settings)
- `scenarios/`: scenario definitions (`steps`, `metrics_definition`)
- `prompts/`: prompt templates for tester and judge roles
- `logs/`: execution logs
- `results/`: machine-readable run results

## Environment separation
- Integration tests use isolated Memgraph services defined in `compose.test.yaml`.
- Test Memgraph endpoint is `bolt://localhost:7697`.
- Runner config `[core]` must define:
  - `memgraph_uri`
  - `memgraph_backup_path`
- Runner config `[core]` may additionally define:
  - `sqlite_backup_path`
- Setup command:
  - `task -t core-rust/Taskfile.yaml integration/prepare`
- Run command:
  - `task -t core-rust/Taskfile.yaml integration/run -- --scenario tests/integration/scenarios/chitchat.yaml --run-count 1`
- Recall-scenario example:
  - `task -t core-rust/Taskfile.yaml integration/run -- --config tests/integration/config/runner.recall.toml --scenario tests/integration/scenarios/conversation_recall_kernel_wording.yaml --run-count 1`
- Help:
  - `task -t core-rust/Taskfile.yaml integration/run -- --help`

## Principles
- Memgraph restore uses latest snapshot through `integration/memgraph/restore/latest`.
- Integration harness restores the snapshot specified by `runner.toml` `core.memgraph_backup_path` before core startup.
- When `core.sqlite_backup_path` is set, integration harness restores canonical `core-rust.db` history into the temp runtime DB before startup.
- `core.sqlite_backup_path` supports:
  - a direct `core-rust.db` file
  - a `.tar.gz` / `.tgz` backup archive containing `./core-rust.db`
- After SQLite restore, integration harness rebuilds `ConversationEvent` recall projections in Memgraph before starting `core-rust`.
- Tester and judge configuration are file-based (not environment-variable based).
- Runtime requires `OPENAI_API_KEY`.
- Scenario text supports secret placeholders:
  - `{{filename}}` resolves from `tests/integration/secrets/filename.age`.
  - Placeholder names allow `[a-zA-Z0-9._-]` only.
  - Missing/invalid placeholder or decrypt failure fails the run.
- Secret decryption key must be provided via environment variable:
  - `PROMPT_PRIVATE_KEY` (X25519 JWK JSON)
- Common baseline metrics:
  - `scenario_requirement_fit` (`0..1`)
  - `dialog_naturalness` (`0..1`)
- Additional metrics are scenario-specific and defined in each scenario.
  - Optional per-metric flag:
    - `exclude_from_pass: true` excludes that metric from `overall_pass` gate evaluation.
    - This does not remove the metric from judge scoring or result `gates`.
    - Baseline metrics (`scenario_requirement_fit`, `dialog_naturalness`) cannot be excluded.

## Scenario steps
- Step mode can define a sequence of conversation and fixed event emission:
  - `steps[].kind: conversation`
  - `steps[].tester_instructions` (required)
  - `steps[].max_turns` (optional, defaults to runner `execution.max_turns`)
  - `steps[].kind: emit_event`
  - `steps[].event.type: trigger` (currently only supported event type)
  - `steps[].event.event` (required event tag name)
  - `steps[].event.payload` (optional JSON object)
  - `steps[].wait_for.tags_any` (optional, defaults to self-improvement processed tags)
  - `steps[].wait_for.timeout_ms` (optional, default `15000`)
- Conversation completion token is fixed to `__TEST_DONE__`.
