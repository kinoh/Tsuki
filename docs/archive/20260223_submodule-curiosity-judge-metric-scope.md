# Submodule Curiosity Judge Scope Tightening

## Context
In integration runs for `submodule_curiosity`, `scenario_requirement_fit` was penalized by factors outside the intended metric scope:
- turn-count / turn-plan completion details,
- assistant or internal event text containing banned words.

The user clarified intended behavior:
- negative constraints should be evaluated on tester/user utterances only,
- turn count is already constrained by harness runtime (`max_turns`) and should not be judged in this metric.

## Decision
- Tighten `scenario_requirement_fit` scope in `tests/integration/scenarios/submodule_curiosity.yaml`:
  - negative constraints are user-turn-only,
  - turn count / turn-plan completion are out of scope for this metric.
- Clarify tester instructions in the same scenario:
  - step 6 closing is optional when run stops within `max_turns`.
- Strengthen judge prompt contract in `tests/integration/prompts/judge.md`:
  - each metric must use only evidence required by that metric definition,
  - no penalties for turn count, turn-plan completion, or assistant/internal text unless explicitly requested by a metric.

## Why
- Aligns scoring behavior with metric intent and harness responsibility split.
- Prevents accidental metric contamination from unrelated evidence.
- Keeps `scenario_requirement_fit` diagnosable for the actual scenario constraints (anchor coverage + user-side negative constraints).
