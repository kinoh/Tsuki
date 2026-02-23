# Submodule Hard-Trigger Self-Inhibition

## Context
- Submodule activations now separate by module in rebuilt snapshots.
- The immediate need is to reduce mistimed repeated hard triggers with minimal complexity.

## Decision
- Add a global hard-trigger self-inhibition mechanism for all submodules.
- Apply inhibition only to hard-trigger selection; soft recommendations remain unchanged.
- Keep implementation in-memory and lightweight.

## Rule
- For each submodule:
  - `effective_hard_score = clamp(base_score - penalty, 0, 1)`
  - `penalty = min(0.36, consecutive_hard_count * 0.12)`
- Selection uses `effective_hard_score` against existing hard threshold.
- After each router step:
  - if hard-triggered: `consecutive_hard_count += 1`
  - if not hard-triggered: streak decays by 1 (to minimum 0)

## Why
- This suppresses repeated hard-trigger bursts without changing soft guidance behavior.
- It avoids additional API/config complexity and keeps behavior observable in router events.
