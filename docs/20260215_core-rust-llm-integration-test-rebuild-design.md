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

### 1. Memgraph restore input is explicit
- The test run must require a backup snapshot file name (not `latest`).
- Rationale: reproducibility across local runs and CI.

### 2. Scenario execution supports repeated runs
- The runner must support multi-run execution for a scenario.
- The run count is not fixed in design; it is provided at execution time.
- Rationale: absorb LLM variance without locking policy too early.

### 3. Scenario schema is minimal and variable-output friendly
- Required scenario fields:
  - `tester_instructions`
  - `metrics_definition`
- Metric outputs are scenario-specific and may differ by scenario.
- Each metric is normalized to `0..1`.
- Rationale: each scenario validates a different concern; fixed metric schema is intentionally avoided.

### 4. Failure on missing output is strict
- If required outputs cannot be obtained (conversation execution failure, timeout without usable trace, judge failure), the scenario is failed.
- Rationale: avoid silent false positives.

### 5. Coarse initial pass threshold
- Initial pass policy is coarse (`score > 0.7` class) and can be refined later.
- Rationale: enable early adoption before policy hardening.

## Runtime Architecture (Proposed)

### Phase A: Environment setup
1. Start/ensure test Memgraph instance.
2. Restore the explicitly specified snapshot file.
3. Create a temporary test config from base `config.toml` with:
   - test-only `db.path` (temporary file),
   - test runtime ports/identifiers as needed.
4. Start `tsuki-core-rust` with the temporary config.

### Phase B: Scenario execution by tester LLM
1. Load scenario (`tester_instructions`, `metrics_definition`).
2. Tester LLM runs dialogue turns through WebSocket client flow.
3. Runtime persists events in libSQL event stream.

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
- No global fixed metric taxonomy across all scenarios.
- No final policy on exact aggregation across repeated runs.
- No optimization policy for token/cost budgets yet.

## Deferred Decisions
These are intentionally postponed and should be finalized during implementation:
- Default event selection policy for judging (`primary-only` vs optional debug inclusion).
- Aggregation rule across repeated runs (mean, min, percentile, gating strategy).
- Common runner-level controls (max turns, timeout profile, retry profile).

## Why This Fits `core-rust/AGENTS.md`
- Keeps behavior event-first: judgment is based on persisted event streams.
- Preserves explicit contracts: minimal required scenario fields are clear.
- Respects WIP test strategy: starts coarse, reproducible, and extensible.
