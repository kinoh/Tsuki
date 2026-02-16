# LLM Integration Tests

This directory contains the rebuilt integration test assets for `core-rust`.

## Layout
- `config/`: runner-level configuration (`tester` and `judge` model/prompt settings)
- `scenarios/`: scenario definitions (`tester_instructions`, `metrics_definition`)
- `prompts/`: prompt templates for tester and judge roles
- `logs/`: execution logs
- `results/`: machine-readable run results

## Environment separation
- Integration tests use isolated Memgraph services defined in `compose.test.yaml`.
- Test Memgraph endpoint is `bolt://localhost:7697`.
- Setup command:
  - `task -t core-rust/Taskfile.yaml integration/prepare`
- Run command:
  - `task -t core-rust/Taskfile.yaml integration/run -- --scenario tests/integration/scenarios/example.yaml --run-count 1`
- Help:
  - `task -t core-rust/Taskfile.yaml integration/run -- --help`

## Principles
- Memgraph restore uses latest snapshot through `integration/memgraph/restore/latest`.
- Tester and judge configuration are file-based (not environment-variable based).
- Runtime requires `OPENAI_API_KEY`.
- Scenario text supports secret placeholders:
  - `{{filename}}` resolves from `tests/integration/secrets/filename.age`.
  - Placeholder names allow `[a-zA-Z0-9._-]` only.
  - Missing/invalid placeholder or decrypt failure fails the run.
- Decrypt key path is configured in `tests/integration/config/runner.toml`:
  - `[secrets].identity_file`
- Common baseline metrics:
  - `scenario_requirement_fit` (`0..1`)
  - `dialog_naturalness` (`0..1`)
- Additional metrics are scenario-specific and defined in each scenario.
