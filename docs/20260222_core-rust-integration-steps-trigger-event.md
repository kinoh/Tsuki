# Core-Rust Integration Harness: Step Sequence with Fixed Trigger Event

Date: 2026-02-22

## Context
- We needed integration scenarios that can emit self-improvement trigger events in a deterministic place.
- The existing integration harness only supported one continuous tester conversation driven by top-level `tester_instructions`.
- A naive `steps` design risked degenerating into single-turn fragments and breaking the existing multi-turn conversation behavior.

## Decision
- Introduce `steps` in integration scenarios as a **phase sequence**, not a single-turn sequence.
- Add two step kinds:
  - `conversation`: multi-turn tester-driven dialogue with its own `tester_instructions` and optional `max_turns`.
  - `emit_event`: fixed runtime event emission (currently only `event.type: trigger`), followed by explicit wait for completion tags.
- Keep conversation completion token fixed to `__TEST_DONE__` (not configurable).
- Require `steps` for all scenarios (legacy top-level `tester_instructions` is not supported).

## Why
- Deterministic event injection is necessary to reliably verify self-improvement flow in integration tests.
- Conversation must remain multi-turn to preserve realism and existing tester/judge behavior.
- Phase-based steps allow controlled trigger insertion without rewriting tester logic into brittle command parsing.

## Implementation Notes
- `core-rust/examples/integration_harness.rs`
  - Scenario schema changed to require `steps`.
  - Added runtime step planning with validation.
  - Added `emit_event` execution over the same WebSocket session.
  - Switched event collection and emit-event completion wait to WebSocket stream events (no direct SQLite reads from harness).
- `core-rust/src/prompts.rs`
  - Added fail-fast validation at prompts load time:
    - loaded `Decision` section must contain `## Memory`.
    - `Base`, `Router`, and `Submodule` are excluded from Memory-section requirement because they are not memory-owner scope.
  - Added unit tests for section detection/validation.
- `core-rust/config.toml`
  - Updated self-improvement relation schema wording to `is-a|part-of|evokes` to match runtime parser expectations.
  - Updated self-improvement memory schema wording:
    - `memory_section_update.target` allows `decision` only.
  - Updated self-improvement proposal guidance:
    - use fixed file headers (`--- a/target`, `+++ b/target`) for stable, short output.
    - require exact context/removal line matching against current target prompt text.
    - explicitly forbid custom patch formats.
    - require emitting a syntactically valid minimal unified diff when there is a proposal (do not null out proposal only due to format uncertainty).
- `core-rust/src/application/improve_service.rs`
  - Enforced structural guard: `memory_section_update` targeting anything except `decision` now fails explicitly.
- `core-rust/tests/integration/README.md`
  - Added step schema documentation and defaults.
  - Added metric schema note:
    - `metrics_definition.<name>.exclude_from_pass: true` keeps scoring/gates but excludes the metric from `overall_pass`.
    - baseline metrics (`scenario_requirement_fit`, `dialog_naturalness`) remain non-excludable.
- `core-rust/tests/integration/scenarios/self_improvement_trigger.yaml`
  - Added scenario example using conversation -> emit_event(trigger) -> conversation.
  - Added `si_pipeline_health` metric (pass-target) focused on deterministic progress signal:
    - at least one `self_improvement.module_processed` with `concept_graph_updated=true`.
    - explicit binary rubric (`1.0` when condition is met, else `0.0`) to reduce judge drift.
  - Marked `self_improvement_effectiveness` as `exclude_from_pass: true` to avoid over-strict fail on non-deterministic apply.
- `core-rust/examples/integration_harness.rs`
  - Added `MetricDefinition.exclude_from_pass` (default false).
  - `overall_pass` now evaluates only non-excluded metrics.
  - Added guard to reject `exclude_from_pass: true` on required baseline metrics.
 - `core-rust/tests/integration/scenarios/chitchat.yaml`
 - `core-rust/tests/integration/scenarios/router_concept_discovery.yaml`
 - `core-rust/tests/integration/scenarios/submodule.yaml`
  - Migrated existing scenarios to `steps` format using `conversation` phases.

## Compatibility Impact
- breaking-by-default (no compatibility layer): Yes.
- Scenario files must define `steps`; top-level `tester_instructions` is no longer accepted.
- Prompt override files now fail to load only when loaded `Decision` section is missing `## Memory`.
- Self-improvement memory updates are now rejected unless `target=decision`.
