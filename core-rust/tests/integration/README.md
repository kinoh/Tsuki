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

## Principles
- Memgraph restore uses latest snapshot through `integration/memgraph/restore/latest`.
- Tester and judge configuration are file-based (not environment-variable based).
- Common baseline metrics:
  - `scenario_requirement_fit` (`0..1`)
  - `dialog_naturalness` (`0..1`)
- Additional metrics are scenario-specific and defined in each scenario.
