# Core Rust test assets

`core-rust` test assets are organized around LLM-driven integration scenarios.

## Test groups
- `tests/integration/`: tester/judge prompts, scenario specs, logs, and machine-readable results.

## Usage
- Prepare integration environment:
  - `task -t core-rust/Taskfile.yaml integration/prepare`
- Run integration harness:
  - `task -t core-rust/Taskfile.yaml integration/run -- --scenario tests/integration/scenarios/chitchat.yaml --run-count 1`
- Show harness help:
  - `task -t core-rust/Taskfile.yaml integration/run -- --help`

See `tests/integration/README.md` for scenario schema and metric/gate rules.
