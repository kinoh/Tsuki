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

## Scope
- In scope:
  - periodic self-improvement cycle
  - event-based trigger records
  - event-based proposal records
  - event-based approval/rejection records
  - event-based apply/rollback records
- Out of scope:
  - separate audit database/service
  - A/B testing
  - additional safety policy framework beyond existing runtime controls

## Event Model
- Every self-improvement action is emitted as an event with standard fields:
  - `ts`, `source`, `modality`, `payload`, `meta.tags`
- Required tags:
  - `improve.trigger`
  - `improve.proposal`
  - `improve.review`
  - `improve.apply`
  - `improve.rollback`
- Suggested payload fields:
  - `phase`: `trigger|proposal|review|apply|rollback`
  - `target`: `base|decision|submodule:<name>`
  - `reason`: short rationale
  - `diff`: proposed patch text (for proposal/apply)
  - `status`: `accepted|rejected|applied|rolled_back`
  - `feedback_refs`: related event ids

## Runtime Flow
1. Scheduler emits `improve.trigger` (daily by default).
2. Reflector modules read recent events and emit `improve.proposal`.
3. Reviewer (human or meta-module) emits `improve.review`.
4. Apply step emits `improve.apply` and updates prompt source.
5. If needed, emit `improve.rollback` and restore prior prompt content.

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

## Debugging
- Debug UI should filter and inspect `improve.*` tags.
- Proposal debugging should show:
  - trigger context
  - proposal diff
  - review outcome
  - apply/rollback transitions
- Because all steps are events, replay and context reconstruction use the same mechanism as normal conversation debugging.

## Why This Works Here
- Preserves architectural coherence: one event fabric for both dialogue and evolution.
- Keeps implementation small and understandable.
- Maximizes model self-awareness of prior attempts and feedback without extra subsystems.
