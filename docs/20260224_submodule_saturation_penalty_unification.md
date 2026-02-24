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
