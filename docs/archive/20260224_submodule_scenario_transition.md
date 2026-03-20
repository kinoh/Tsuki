# Submodule Scenario Transition: Curiosity to Self-Preservation

## Decision
Replaced `core-rust/tests/integration/scenarios/submodule.yaml` with a fixed 5-turn Japanese scenario that explicitly transitions from curiosity-oriented content to self-preservation-oriented operational concerns.

## Why
The previous scenario mixed intents (neutral/lightweight/safety) in a way that made it hard to evaluate whether submodules trigger at the right timing and independently.
The new scenario introduces a deliberate pivot:
- turns 1-2: concept graph and experience expansion (curiosity-heavy)
- turn 3: transition pressure (context growth)
- turns 4-5: operational stability and cost/hidden load (self-preservation-heavy)

This structure allows judging both timing correctness and non-overtriggering in one scenario.

## Scope
Updated only scenario content and metric descriptions in:
- `core-rust/tests/integration/scenarios/submodule.yaml`

No interface/schema change was introduced.
