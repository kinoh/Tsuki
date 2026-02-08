# Self-Improvement Phase (Event-Native Design)

## Context
- The system is not a business product and does not require separate security-grade audit rails.
- The model should continuously know what it attempted, what feedback happened, and why updates were proposed.
- Improvement trigger signals and proposal artifacts should be first-class events in the same event stream.

## Design Principles
- Use one event system for conversation and self-improvement state transitions.
- Do not introduce a separate audit subsystem.
- Keep reflection and update proposals observable by the same runtime context model.
- Keep safety/A-B mechanisms out of scope for now.
- Treat approval as the semantic commit point: once approved, the system is expected to converge to an applied state.

## Scope
- In scope:
  - periodic self-improvement cycle
  - event-based trigger records
  - event-based proposal records
  - event-based approval/rejection records
- Out of scope:
  - separate audit database/service
  - A/B testing
  - additional safety policy framework beyond existing runtime controls
  - dedicated apply/rollback event families
  - delayed projection reconciliation workflows

## Event Model
- Every self-improvement action is emitted as an event with standard fields:
  - `ts`, `source`, `modality`, `payload`, `meta.tags`
- Required tags:
  - `improve.trigger`
  - `improve.proposal`
  - `improve.review`
- Suggested payload fields:
  - `phase`: `trigger|proposal|review`
  - `target`: `base|decision|submodule:<name>`
  - `reason`: short rationale
  - `diff`: proposed patch text
  - `status`: `pending|approved|rejected`
  - `feedback_refs`: related event ids
  - `review`: `approval|rejection` (for review phase)

## Event Semantics
- `improve.review` with `review=approval` means the proposal is logically accepted and should be reflected in prompt state.
- If reflection fails, emit a standard runtime `error` event with improvement context (no dedicated `projection.error` event type).
- Event sourcing expectation: eventual consistency from approved proposal to materialized prompt content.

## Runtime Flow
1. Scheduler emits `improve.trigger` (daily by default).
2. Reflector modules read recent events and emit `improve.proposal`.
3. Reviewer (human or meta-module) emits `improve.review`.
4. Prompt projection updates persisted prompt material from approved proposals.
5. On projection failure, runtime emits a normal `error` event and keeps proposal/review events as source of truth.

## Module Responsibilities
- Submodule reflector:
  - proposes updates only for its own submodule prompt.
- Decision reflector:
  - proposes updates only for decision prompt.
- Base reflector:
  - proposes updates only for base prompt.
- Meta reviewer:
  - does not directly rewrite prompts;
  - evaluates proposal consistency and emits review events.

## Trigger Strategy
- Primary trigger: periodic run (daily).
- Optional immediate trigger: explicit high-signal user feedback.
- Both are represented as `improve.trigger` events.

## Approval Strategy
- Approval is represented in-stream via `improve.review` events.
- No separate approval ledger is required.
- Final source of truth is the event stream plus current prompt files.
- `review=approval` is not optional metadata; it is the intent that the projection layer must realize.

## Debugging
- Debug UI should filter and inspect `improve.*` tags.
- Proposal debugging should show trigger context, proposal diff, and review outcome.
- Improvement trigger action should be initiated from the right panel after selecting the relevant module, to stay consistent with existing module run interactions.
- Because all steps are events, replay and context reconstruction use the same mechanism as normal conversation debugging.

## Why This Works Here
- Preserves architectural coherence: one event fabric for both dialogue and evolution.
- Keeps implementation small and understandable.
- Maximizes model self-awareness of prior attempts and feedback without extra subsystems.
