# Core Rust LLM-Driven Integration Test Rebuild Design

## Context
The current `core-rust` E2E tooling (`test_runner` + `ws_scenario`) records JSONL logs for manual review, but does not provide automated evaluation.

The rebuild goal is:
- restore test Memgraph state before execution,
- run scenarios against an isolated temporary libSQL file (`config.db.path`),
- use an LLM-based tester to drive conversation,
- define per-scenario achievement requirements,
- evaluate event streams with a separate LLM judge,
- produce both binary pass/fail and quantitative metrics.

## Stable Decisions

### 1. Memgraph restore policy uses latest snapshot
- Integration test setup restores Memgraph using `integration/memgraph/restore/latest`.
- Snapshot file name is not passed through test runner/task arguments.
- The restore target is isolated test Memgraph (`compose.test.yaml`, `memgraph-test`, `bolt://localhost:7697`).
- Rationale: keep outer orchestration simple for local-first test operation.

### 2. Scenario execution supports repeated runs
- The runner must support multi-run execution for a scenario.
- The run count is not fixed in design; it is provided at execution time.
- Rationale: absorb LLM variance without locking policy too early.

### 2.1 Integration assets location
- LLM-driven integration test assets are stored under `core-rust/tests/integration/`.
- Existing manual scenario client assets under `core-rust/tests/client/` remain separate.
- Rationale: avoid mixing manual WebSocket scripts and judge-based integration assets.

### 3. Scenario schema is minimal and variable-output friendly
- Required scenario fields:
  - `tester_instructions`
  - `metrics_definition`
- A minimal common metric baseline is required across scenarios:
  - `scenario_requirement_fit` (`0..1`)
  - `dialog_naturalness` (`0..1`)
- Metric outputs can add scenario-specific metrics beyond the common baseline.
- Each metric is normalized to `0..1`.
- Rationale: every scenario validates a different concern, but baseline comparability is still needed.

### 4. Failure on missing output is strict
- If required outputs cannot be obtained (conversation execution failure, timeout without usable trace, judge failure), the scenario is failed.
- Rationale: avoid silent false positives.

### 5. Coarse initial pass threshold
- Initial pass policy is coarse (`score > 0.7` class) and can be refined later.
- Shared pass/fail gating treats common metrics independently (AND):
  - `mean(scenario_requirement_fit) > 0.7` and `min(scenario_requirement_fit) > 0.5`
  - `mean(dialog_naturalness) > 0.7` and `min(dialog_naturalness) > 0.5`
- Rationale: enable early adoption before policy hardening.

### 6. Judge event input policy
- Default judge input uses `primary events` only.
- Scenarios may opt in to include debug events via `include_debug_events: true`.
- Rationale: keep default evaluation tied to runtime semantics while preserving optional observability-based checks.

### 7. Repeat-run aggregation policy
- Use per-metric `mean` and `min` only.
- Evaluate pass/fail with independent AND gates for common metrics.
- Do not use weighted metric mixing in the initial design.
- Rationale: preserve semantic separability between metric dimensions.

### 8. Failure typing policy
- Result artifacts use string error codes (not numeric code tables).
- Initial error set:
  - `EXEC_TIMEOUT`
  - `EXEC_WS_ERROR`
  - `JUDGE_ERROR`
  - `INVALID_OUTPUT`
- Rationale: simple, explicit, and extensible failure reporting without premature schema complexity.

## Runtime Architecture (Proposed)

### Phase A: Environment setup
1. Start/ensure isolated test Memgraph instance (`compose.test.yaml`, `memgraph-test`).
2. Restore latest snapshot (`integration/memgraph/restore/latest`).
3. Create a temporary test config from base `config.toml` with:
   - test-only `db.path` (temporary file),
   - `MEMGRAPH_URI=bolt://localhost:7697`,
   - test runtime ports/identifiers as needed.
4. Start `tsuki-core-rust` with the temporary config.

### Phase B: Scenario execution by tester LLM
1. Load scenario (`tester_instructions`, `metrics_definition`).
2. Load tester/judge model and prompt from file-based runner config (not env vars).
3. Tester LLM runs dialogue turns through WebSocket client flow.
4. Runtime persists events in libSQL event stream.

### Phase C: Evaluation by independent judge LLM
1. Read produced event stream from the test DB.
2. Feed scenario definitions + event stream to judge LLM.
3. Judge outputs:
   - binary verdict,
   - scenario-defined quantitative metrics (`0..1`),
   - concise evidence/reasoning summary.

### Phase D: Result packaging
- Persist machine-readable result artifact per run (JSON).
- Include snapshot id, scenario id, run index, verdict, metrics, and failure type if any.

## Non-Goals (Current Design Stage)
- No large fixed global taxonomy beyond the common baseline metrics.
- No final policy on exact aggregation across repeated runs.
- No optimization policy for token/cost budgets yet.

## Why This Fits `core-rust/AGENTS.md`
- Keeps behavior event-first: judgment is based on persisted event streams.
- Preserves explicit contracts: minimal required scenario fields are clear.
- Respects WIP test strategy: starts coarse, reproducible, and extensible.
