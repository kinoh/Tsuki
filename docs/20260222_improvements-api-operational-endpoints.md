# Improvements API Operational Endpoints and Approval IDs

## Context
The self-improvement flow had only debug endpoints and an older payload contract (`section` / `content`).
The latest prompt-diff schema requires auditable proposal/review/apply events and stable proposal identifiers.

## Decision
- Move self-improvement HTTP routes to domain-neutral ingress paths:
  - `POST /triggers`
  - `POST /proposals`
  - `POST /reviews`
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
- `trigger_ingress_api` (`/triggers`): emits `self_improvement.triggered` only.
- `improve_service` (trigger consumer and LLM/tool execution path):
  - consumes `self_improvement.triggered` from the in-process event stream.
  - emits `self_improvement.module_processed` once per resolved module target.
  - emits `self_improvement.trigger_processed` once as the aggregate result (debug-only tag attached to keep it out of prompt history).
  - `self_improvement.trigger_processed` payload includes:
    - `status`: `success|partial|failed`
    - `memory_updated`: boolean
    - `concept_graph_updated`: boolean
    - `proposal_ids`: optional array (`proposal` was generated when present)
    - `error_code`, `error_detail`: required on `failed`, optional on `partial`
- `improve_approval_service` keeps one essential event per action:
  - `self_improvement.proposed`
  - `self_improvement.reviewed`
  - `self_improvement.applied`

### Simplification
- `proposal_created` is intentionally not introduced.
- Proposal creation is derived from the presence of `proposal_ids` in `self_improvement.trigger_processed`.

## Implementation Follow-up (2026-02-22)
- Trigger runtime execution now lives in `core-rust/src/application/improve_service.rs` and starts from an event consumer (`start_trigger_consumer`).
- `trigger_ingress_api` is responsible only for writing `self_improvement.triggered`; it does not start execution directly.
- Proposal/review/apply logic and prompt-diff mutation rules live in `core-rust/src/application/improve_approval_service.rs`.
- Submodule concept existence is now guaranteed in module-worker post-LLM execution:
  - when `module_target` is `submodule:<name>`, runtime always runs `concept_upsert("submodule:<name>")` before applying plan actions.
  - on ensure failure, the module result is emitted as failed with `error_code=SUBMODULE_CONCEPT_ENSURE_FAILED`.
- Trigger worker instructions are moved from code hardcoding to `config.toml` (`[prompts].self_improvement_trigger_instructions`):
  - Why: avoid hidden fallback behavior that silently changes runtime semantics.
  - Policy: for self-improvement worker prompt, runtime reads configured text directly instead of constructing defaults in code.
- Self-improvement worker input now includes `recent_event_history` in the same text format used by normal module execution (`ts | role | message`):
  - Why: scheduled automatic runs should rely on runtime event history (including user reactions) as first-class signal, not only manual `feedback_refs`.
  - `feedback_refs` remains optional supplemental hints.

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
