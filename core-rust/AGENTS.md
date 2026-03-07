# AGENTS.md for `core-rust`

This file is a practical guide for contributors working in `core-rust`.
It captures stable implementation rules and clearly marks active WIP areas.

## Scope and intent
- `core-rust` is event-first and router-first.
- Keep behavior observable through persisted events.
- Prefer explicit contracts over implicit behavior.
- `core-rust` is under development and has not been deployed; it requires absolutely no backward compatibility.
- Backward-compatibility layers are prohibited by default in `core-rust`.
- Avoid dual paths (`old/new`), compatibility flags, and migration-only fallbacks unless explicitly required by a new written decision.

## Terms

### `normal flow`
- Definition: the standard runtime path for regular user input handling.
- Owned by: Application orchestration.
- Must not: unconditionally run all submodules for each input.

### `hard trigger`
- Definition: a submodule execution request selected and executed in Router path before Decision.
- Owned by: Router policy/runtime.
- Must not: be re-computed in Decision stage.

### `soft recommendation`
- Definition: a candidate submodule hint derived from activation context.
- Owned by: Router policy/runtime.
- Must not: be interpreted as mandatory execution.

### `activation query terms`
- Definition: minimal query-oriented terms produced by Router for concept activation lookup.
- Owned by: Router.
- Must not: include unrelated narrative output.

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
- Output: `activation_query_terms`, activation concepts, hard/soft trigger results.
- Router performs concept-graph query in-process.
- Router owns hard/soft trigger policy.
- Router executes hard-triggered submodules before Decision.
- Router LLM is the only component that selects recall seeds.
- Downstream modules must not re-score, re-rank, or re-interpret seed relevance.
- Router prioritizes latency far above stability/certainty-oriented retries in normal flow.

#### Application orchestration (`pipeline_service` and related application layer)
- Invokes Router and consumes Router output.
- Composes decision input context.
- Runs Decision with Router-prepared activation and hard-trigger outputs.
- Must not add ad-hoc semantic scoring that re-derives concept relevance.

#### Decision module
- Receives composed context and available tools.
- Decides whether to call submodule tools.
- Emits decision/action outputs as events.

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
- Keep API endpoints as ingress contracts; orchestration belongs to domain/application services.
- Keep ingress contracts minimal; avoid carrying manual-only hint fields in operational trigger contracts.
- WebSocket control input must be explicit and allowlisted by `type` (e.g., `trigger`); do not allow arbitrary event injection payloads.

### Event definition discipline
- Event-definition requirements must be made explicit in design docs.
- When adding or changing events, document at least:
  - owner module
  - target domain boundary (who consumes it and why)
  - producer and expected consumers
  - whether it is primary runtime context input or debug-only observability
- If these points cannot be stated clearly, do not add the event.
- Only events that are necessary for model reasoning should be included in LLM input history.
- Aggregate/operational events that do not improve reasoning should be debug-only and excluded from prompt history.

## Runtime invariants
- Submodule execution in normal flow is demand-driven, not unconditional.
- Persist primary runtime semantics in event stream.
- Keep debug observability events available, but exclude debug-tagged events from model input history.
- Keep decision context structure configurable through `config.toml` template fields.

### Event stream semantics
- Event stream follows an Event Storming-style domain-event observability model, not a causally strict transaction log.
- It is intentionally non-transactional for ordering guarantees; consumers must not require strict total ordering as a runtime contract.
- Event order, timing, and adjacency are not guaranteed to encode exact request/response causality.
- Missing, delayed, duplicated, or out-of-order observations must be treated as valid stream behavior unless a specific API contract states otherwise.
- Runtime behavior must not rely on stream order as a control-plane guarantee; stream consumers should interpret events as best-effort facts.

## Configuration policy
- Non-secret runtime settings belong in `config.toml`.
- Secrets belong to environment variables.
- Missing required config should fail fast.
- Thresholds and context-template wording should be tuned by config, not code rewrites.
- Do not hardcode prompt text in Rust source; prompt/context wording must be owned by config or prompt files.

## MCP bootstrap policy
- Initial MCP trigger onboarding should prefer generic action-family concepts, not downstream use-case examples.
- Keep bootstrap trigger-policy code generic; do not add per-tool hardcoded overrides in shared trigger-generation modules.
- If an MCP tool description or schema wording is the source of bad trigger generation, fix the provider contract first; do not patch around it in `core-rust`.
- Enforce bootstrap safety mechanically where needed (for example count caps), but keep semantic steering in shared rules rather than tool-specific exceptions.

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
- If implementation strictly follows an existing design decision without adding interpretation or policy change, do not add a new decision doc.
- Add/update docs only when introducing, changing, or clarifying decisions (including conflict resolution).
- Responsibility-boundary changes and event/API contract changes require same-day docs updates.
- For `core-rust` design/implementation docs, include a short `Compatibility Impact` statement:
  - default expectation: `breaking-by-default (no compatibility layer)`.
  - if compatibility is introduced, the document must justify why replacement/removal was not acceptable.
- Keep one decision goal in one document. If several code changes serve the same design goal, extend the existing same-day doc instead of splitting it into local implementation fragments.

## Test-scope separation
- Treat scenario-spec changes and test-harness changes as different scopes.
- Scenario scope: files under `tests/integration/scenarios/*.yaml` only (metric definitions, tester instructions, scenario intent).
- Harness scope: runner/judge/execution behavior under `examples/integration_harness.rs` and related runtime tooling.
- Do not mix scenario and harness edits in one step without explicit confirmation.
- When asked to improve a scenario, default to scenario-file-only edits unless the user explicitly asks to change test mechanism behavior.

## Key references
- `docs/20260131_thinking-core-rust-design.md`
- `docs/20260212_router-concept-activation-core-rust-implementation.md`
- `docs/20260213_router-concept-graph-interface-and-responsibility-clarification.md`
- `docs/20260213_router-concept-graph-core-rust-implementation-log.md`
- `docs/20260215_router-responsibility-shift-to-integrated-routing.md`
- `docs/20260214_decision-input-context-template-config.md`
- `docs/20260214_always-on-debug-observability.md`
- `docs/20260209_event-log-redefinition-and-debug-worklog-role.md`
