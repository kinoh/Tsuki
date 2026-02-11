# Router-First Concept Activation and Submodule Gating

## Context
- Current runtime executes all active submodules for each user input in the normal flow (`run_submodules` before decision).
- Decision input is currently composed as formatted event-history text, not as a structured state object.
- This behavior is expensive and does not match human-like response patterns, where deep deliberation is conditional.

## Problem
- Always-on submodule execution increases latency and token usage.
- Decision receives mixed textual history without explicit separation between:
  - factual past events,
  - current internal state,
  - activation intent.
- In debug scenarios, manually provided submodule outputs can duplicate history semantics if treated as appended events.

## Design Goal
- Make the router the first activation stage.
- Use concept graph excitation as the shared intermediate representation.
- Execute submodules conditionally via concept-node hooks.
- Keep Decision input format close to production while adding explicit, minimal state for current activation.

## Core Proposal
1. Router-first activation
- Router consumes new user input and updates concept graph excitation.
- Router emits two activation channels:
  - `hard_trigger`: high-threshold forced submodule execution.
  - `soft_recommend`: low-threshold recommendation for decision context.

2. Submodule as concept-backed nodes
- Each submodule is represented by one or more concept nodes in the graph.
- Hook rule:
  - if excitation >= hard threshold, submodule must run.
  - if soft threshold <= excitation < hard threshold, submodule is recommended but not required.

3. Decision input split
- Decision should continue receiving event history text for "what happened".
- Additional state should be passed as a compact activation block for "what is active now":
  - top excited concepts (bounded list),
  - hard triggers,
  - soft recommendations.

## Decision Input Contract (target)
- `Recent event history` (existing production-like textual format)
- `Activation state` (new compact section)
  - `active_concepts`: top N `{name, score}`
  - `hard_triggers`: list of submodule names
  - `soft_recommendations`: list of submodule names

Notes:
- Keep activation section size bounded (for example top 5-10 concepts).
- Do not append synthetic events for user-provided submodule outputs.
- If debug provides `submodule_outputs`, apply as in-memory overrides over matching submodule rows in composed history.

## Execution Semantics
- Normal runtime
  - Router updates excitation.
  - Execute hard-triggered submodules only.
  - Decision receives:
    - event history,
    - hard-trigger outputs (if any),
    - activation state including soft recommendations.
- Decision may choose:
  - immediate reply,
  - follow-up question,
  - optional additional submodule call (if recommended and uncertainty is high).

## Threshold Strategy
- Two thresholds per submodule concept hook:
  - `T_hard` (force execution)
  - `T_soft` (recommendation)
- Constraints:
  - `T_soft < T_hard`
  - both are configurable and observable in logs.
- Initial rollout should prioritize conservative hard triggers and measurable soft recommendations.

## Logging and Observability
- Persist router activation artifacts as first-class events (non-debug):
  - activation snapshot event with top concepts and scores.
  - trigger decision event with hard/soft lists.
- Keep `debug,llm.raw` for raw prompt/response inspection.
- Event Log remains the source of truth for replay and cutoff/exclude controls.

## Migration Plan
1. Introduce activation snapshot event schema.
2. Add router threshold evaluation and hard/soft output.
3. Gate submodule execution by hard triggers in normal runtime.
4. Extend decision context composer with activation block.
5. Add metrics and tune thresholds with real traffic traces.

## Risks
- Under-triggering can reduce response quality.
- Over-triggering can regress to near current cost.
- Concept-node mapping quality directly affects submodule selection.

Mitigations:
- Start with low-risk hard-trigger set.
- Track trigger hit-rate, decision reversals, and latency deltas.
- Iterate threshold values with explicit metrics.

## Explicit Notes from User Feedback
- Executing all submodules on every input is considered overuse of resources.
- Human-like behavior should not assume constant deep deliberation.
- Decision input should remain production-like; avoid adding noisy ad hoc sections.
- User-provided submodule outputs should override matching history semantics in-memory, not be persisted as duplicate events.

## Non-Goals
- Full concept normalization/synonym resolution in this phase.
- Large schema migration of existing event store.
- Immediate replacement of all decision prompt templates.
