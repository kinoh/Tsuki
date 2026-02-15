# AGENTS.md for `core-rust`

This file is a practical guide for contributors working in `core-rust`.
It captures stable implementation rules and clearly marks active WIP areas.

## Scope and intent
- `core-rust` is event-first and router-first.
- Keep behavior observable through persisted events.
- Prefer explicit contracts over implicit behavior.
- `core-rust` is under development and has not been deployed; it requires absolutely no backward compatibility.

## Terms

### `normal flow`
- Definition: the standard runtime path for regular user input handling.
- Owned by: Application orchestration.
- Must not: unconditionally run all submodules for each input.

### `hard trigger`
- Definition: a submodule execution request that Application must run before Decision.
- Owned by: Application orchestration policy.
- Must not: be treated as a Router-owned execution decision.

### `soft recommendation`
- Definition: a candidate submodule hint derived from activation context.
- Owned by: Application orchestration policy.
- Must not: be interpreted as mandatory execution.

### `activation query terms`
- Definition: minimal query-oriented terms produced by Router for concept activation lookup.
- Owned by: Router.
- Must not: include trigger policy or tool execution.

### `decision context`
- Definition: fact-style input assembled for Decision from latest input, activation data, immediate outputs, and history.
- Owned by: Application orchestration.
- Must not: include hidden guidance that belongs to prompt instructions.

### `primary events`
- Definition: persisted runtime semantic events used as source-of-truth for behavior and context control.
- Owned by: Runtime event model.
- Must not: depend on debug-only view artifacts.

### `debug events`
- Definition: observability events for diagnostics (`llm.raw`, `llm.error`, `concept_graph.query`, etc.).
- Owned by: Runtime observability behavior.
- Must not: be included in model input history.

### `Event Log`
- Definition: primary debug UI stream over persisted events.
- Owned by: Debug UI/event query layer.
- Must not: be scoped to a derived subset as default behavior.

### `Work Log`
- Definition: optional/legacy debug UI view term.
- Owned by: Debug UI presentation only.
- Must not: define runtime semantics or context-control semantics.

## Responsibility boundaries

### Top-level Modules

#### Router
- Input: latest user text.
- Output: query-oriented activation terms.
- Router must not execute tools.
- Router must not own hard/soft trigger policy.

#### Application orchestration (`pipeline_service` and related application layer)
- Reads concept-graph activation state via in-process interfaces.
- Applies hard/soft trigger policy.
- Application executes submodules only via hard triggers or decision-requested tool calls.
- Composes decision input context.
- Executes hard triggers before decision when configured.
- Must not add ad-hoc semantic scoring that re-derives concept relevance.

#### Decision module
- Receives composed context and available tools.
- Decides whether to call submodule tools.
- Emits decision/question/action outputs as events.

#### Submodules
- Execute only when explicitly invoked (hard trigger stage or decision tool call).
- May perform concept-graph mutations according to module purpose.

### Basic Functions

#### Concept-graph access
- Activation-critical path must use in-process access.
- Do not depend on MCP transport round-trip latency for activation path.
- Behavior should stay compatible with concept-graph MCP contracts.

#### Transport layer (`main.rs` routes/ws handlers)
- Keep handlers thin (validation + delegation + response mapping).
- Keep business rules in application services, not in transport handlers.

## Runtime invariants
- Submodule execution in normal flow is demand-driven, not unconditional.
- Persist primary runtime semantics in event stream.
- Keep debug observability events available, but exclude debug-tagged events from model input history.
- Keep decision context structure configurable through `config.toml` template fields.

## Configuration policy
- Non-secret runtime settings belong in `config.toml`.
- Secrets belong to environment variables.
- Missing required config should fail fast.
- Thresholds and context-template wording should be tuned by config, not code rewrites.

## WIP areas

### Work Log in debug_ui
- Direction is to treat Event Log as the primary concept.
- `Work Log` is considered a UI/view term and should not drive runtime semantics.
- Avoid introducing new behavior that depends on `worklog`-specific event assumptions.

### Persistence usage policy (libSQL)
- libSQL remains the current persistence backend for events/state/modules.
- Operational policy for prompt-level usage of stored state is not finalized.

### Self-improvement flow
- Current flow exists, but redesign is expected after concept-graph integration direction is refined.
- Do not over-expand self-improvement APIs or schemas without a new design decision record.

### Test strategy
- E2E/manual scenario tooling exists but test policy is still maturing.

## Change discipline
- When changing stable rules in this file, add a dated decision note under `docs/`.
- If a rule is not stable yet, place it under `WIP` instead of presenting it as fixed policy.
- If documents conflict, prefer the newest explicit clarification doc and record the reconciliation.

## Key references
- `docs/20260131_thinking-core-rust-design.md`
- `docs/20260212_router-concept-activation-core-rust-implementation.md`
- `docs/20260213_router-concept-graph-interface-and-responsibility-clarification.md`
- `docs/20260213_router-concept-graph-core-rust-implementation-log.md`
- `docs/20260214_decision-input-context-template-config.md`
- `docs/20260214_always-on-debug-observability.md`
- `docs/20260209_event-log-redefinition-and-debug-worklog-role.md`
