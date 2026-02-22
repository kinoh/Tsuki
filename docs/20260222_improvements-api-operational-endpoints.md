# Improvements API Operational Endpoints and Approval IDs

## Context
The self-improvement flow had only debug endpoints and an older payload contract (`section` / `content`).
The latest prompt-diff schema requires auditable proposal/review/apply events and stable proposal identifiers.

## Decision
- Move self-improvement HTTP routes from debug paths to operational paths:
  - `POST /improvements/trigger`
  - `POST /improvements/proposal`
  - `POST /improvements/review`
- Use `proposal_id = proposal event_id` (no additional identifier layer).
- Enforce one review per proposal (`proposal_id`) in runtime.
- Emit event families aligned to the latest contract intent:
  - `self_improvement.proposed`
  - `self_improvement.reviewed`
  - `self_improvement.applied`

## Agreed Event Policy (2026-02-22)
- Principle: each module emits one essential event for one processing unit.
- Optional debug observability (for example `debug,llm.raw`) can be emitted separately and must not be treated as contract events.

### Module responsibilities
- `improve_service` (`/improvements/trigger`): emits `self_improvement.triggered` only.
- Trigger worker (LLM/tool execution path): emits `self_improvement.trigger_processed` only.
  - `self_improvement.trigger_processed` payload includes:
    - `status`: `success|partial|failed`
    - `memory_updated`: boolean
    - `concept_graph_updated`: boolean
    - `proposal_id`: optional (`proposal` was generated when present)
    - `error_code`, `error_detail`: required on `failed`, optional on `partial`
- `improve_service` proposal/review/apply path keeps one essential event per action:
  - `self_improvement.proposed`
  - `self_improvement.reviewed`
  - `self_improvement.applied`

### Simplification
- `proposal_created` is intentionally not introduced.
- Proposal creation is derived from the presence of `proposal_id` in `self_improvement.trigger_processed`.

## Why
- Operational endpoints remove unnecessary debug-path coupling for this flow.
- Reusing event IDs avoids redundant identifier management and keeps traceability direct.
- One-review enforcement gives deterministic approval state transitions.

## Implementation Notes
- Proposal request moved to `target + job_id + diff_text` with `requires_approval=true` validation.
- Prompt apply path now validates and applies unified diff text deterministically (non-LLM).
- Apply emits success/failure events with explicit status payload.
- Existing debug UI was minimally updated to call `/improvements/*` and send the new fields.

## Compatibility Impact
breaking-by-default (no compatibility layer)
