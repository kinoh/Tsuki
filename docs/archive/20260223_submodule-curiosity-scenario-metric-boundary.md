# Submodule Curiosity Scenario Metric Boundary

## Context
When drafting a new curiosity-focused integration scenario, metric responsibilities overlapped:
- anchor-word and forbidden-topic checks,
- trigger correctness,
- dialogue quality.

This made scoring harder to interpret because scenario-specific checks were mixed with cross-scenario quality checks.

## Decision
For `core-rust/tests/integration/scenarios/submodule_curiosity.yaml`:
- Keep `scenario_requirement_fit` focused on scenario-specific constraints only:
  1. required anchor words are present (`概念グラフ`, `ワクワク`, `記憶`)
  2. forbidden-topic drift is absent (safety/rest and privacy/security keywords)
- Keep trigger behavior in dedicated metrics:
  - `submodule_trigger_precision`
  - `submodule_trigger_recall`
- Keep flow quality in `dialog_naturalness`.

## Why
- Prevent duplicated scoring criteria across metrics.
- Make failures diagnosable by responsibility:
  - requirement mismatch,
  - trigger mismatch,
  - dialogue quality.
- Improve reproducibility of scenario interpretation across runs.
