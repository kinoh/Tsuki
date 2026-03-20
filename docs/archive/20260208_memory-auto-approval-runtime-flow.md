# Memory Auto-Approval Runtime Flow (core-rust)

## Context
- The self-improvement design was agreed to be implemented in `core-rust` only.
- Scheduler-driven daily execution is intentionally out of scope for this phase.
- Proposals should use event-native semantics with approval as the projection commit point.
- `Memory` section updates should bypass manual approval while preserving event consistency.

## Decisions
- Added debug-only self-improvement endpoints:
  - `POST /debug/improve/trigger`
  - `POST /debug/improve/proposal`
  - `POST /debug/improve/review`
- Added event lookup by id in the event store to support review by `proposal_event_id`.
- Proposal payload contract for runtime:
  - `target`: `base|decision|submodule:<name>`
  - `section`: section name (`Memory` is special)
  - `content`: replacement content
- Automatic approval behavior:
  - if `section == "Memory"`, runtime emits `improve.review(review=approval)` automatically;
  - projection runs immediately after that review event.
- Projection behavior:
  - `section == "Memory"`: replace only the `Memory` markdown section body in target prompt text;
  - otherwise: replace full target prompt text with `content`.
- Success projection event is not emitted in this phase.
- On projection failure, runtime emits a normal `error` event with related review `event_id`.

## Why
- Keeps one event fabric for trigger/proposal/review/projection outcomes.
- Preserves manual control for non-memory prompt updates.
- Keeps auto-approval narrow and explicit (`Memory` heading exact match).
- Avoids introducing new storage or approval ledgers while keeping replay/debug observability.

## Debug UI integration
- Added a Self-Improvement control section for trigger/propose/review actions.
- Added worklog entries for improve actions through existing `debug,worklog` stream.
- Proposal/review details are visible in the existing Output panel.
