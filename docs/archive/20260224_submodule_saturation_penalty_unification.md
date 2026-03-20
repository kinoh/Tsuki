# Submodule Saturation Penalty Unification

## Decision
Replaced router-side hard streak inhibition with unified saturation penalty semantics and integrated post-hard activation dampening into concept graph state updates.

## Why
The previous approach separated two concerns:
- hard selection inhibition in router memory (`hard_streak`)
- no direct concept-graph feedback after hard execution

That made repeated high activation hard to control while keeping concept graph as the single source of activation truth.

The new design keeps natural decay unchanged and introduces behavior-specific self-inhibition for action-like submodule nodes.

## What changed
- `AppState` now stores per-module saturation levels (`HashMap<String, f64>`) instead of hard streak counters.
- Router computes `hard_effective_scores = raw_score - saturation_penalty`.
- After hard trigger execution:
  - saturation level is updated (increase for fired modules, recover for others)
  - the corresponding `submodule:*` concept arousal is dampened via concept graph API
- Router debug output now exposes `saturation_penalties`.

## Scope
- `core-rust/src/main.rs`
- `core-rust/src/application/router_service.rs`
- `core-rust/src/activation_concept_graph.rs`

No compatibility layer was kept by request.

## 2026-02-24 Parameter tuning (goal + numeric basis)
- Goal for this scenario:
  - stop `curiosity` hard-trigger in turns 4-5 (post-pivot to ops/cost context)
  - keep `self_preservation` available at least as soft recommendation
- Fixed hard threshold:
  - `hard_trigger_threshold = 0.85`
- Observed failure points from recent runs:
  - `max6` turn4: `hard_effective_scores.curiosity = 0.999808` (hard fired)
  - `max10` turn4: `hard_effective_scores.curiosity = 0.959998` (hard fired)
  - worst-case extra suppression needed at turn4: `0.999808 - 0.85 = 0.149808`
- Penalty model approximation across one non-hard turn:
  - effective carry-over penalty is roughly `SATURATION_STEP - SATURATION_RECOVERY`
  - requirement: `SATURATION_STEP - SATURATION_RECOVERY >= 0.15`
- Chosen parameters:
  - `SATURATION_STEP = 0.24`
  - `SATURATION_RECOVERY = 0.06`
  - `SATURATION_MAX = 0.72`
  - `POST_HARD_DAMPEN_RATIO = 0.35`
- Why these values:
  - `0.24 - 0.06 = 0.18` gives +0.03 safety margin above the minimum 0.15 requirement.
  - higher `SATURATION_MAX` avoids early clipping during repeated hard-trigger phases.
  - stronger post-hard dampening is added to reduce immediate re-ignition of `submodule:*` arousal.
