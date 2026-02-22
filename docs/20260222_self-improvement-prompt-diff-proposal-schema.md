# Self-Improvement Proposal Schema for Prompt Diff

## Context
The self-improvement flow is intended for real operation, not only debug usage.
For prompt improvement, proposal/review/apply events must be explicit and auditable.
Apply must be non-LLM and deterministic.

The proposal payload should stay minimal and strict.
The proposal content is defined as prompt diff text only.

## Decision
Use a text-only unified diff as the proposal content format for prompt improvements.

### Canonical proposal content
- Field name: `diff_text`
- Type: string
- Semantics: unified diff against the current prompt text of the proposal target
- Encoding: UTF-8 text

### Target scope
`target` must be one of:
- `base`
- `router`
- `decision`
- `submodule:<name>`

### Proposal event contract (prompt improvements)
Required payload fields:
- `proposal_id`: stable identifier
- `job_id`: identifier of the triggering improvement job
- `target`: prompt target
- `diff_text`: unified diff text only
- `requires_approval`: must be `true` for prompt improvements
- `created_by`: module/worker identity
- `created_at`: RFC3339 timestamp

No structured operation list is used for prompt proposals.
No non-text diff representation is used.

### Review event contract (`self_improvement.reviewed`)
Required payload fields:
- `proposal_id`: identifier of the reviewed proposal
- `job_id`: identifier of the originating improvement job
- `target`: same prompt target as proposal
- `decision`: `approved|rejected`
- `reviewed_by`: reviewer identity
- `review_reason`: short rationale text
- `reviewed_at`: RFC3339 timestamp

Rules:
- Exactly one review event is expected per proposal.
- Only `decision=approved` can proceed to apply.

### Apply event contract (`self_improvement.applied`)
Required payload fields:
- `proposal_id`: identifier of the applied proposal
- `job_id`: identifier of the originating improvement job
- `target`: same prompt target as proposal
- `status`: `success|failed`
- `applied_by`: applier identity (runtime worker)
- `applied_at`: RFC3339 timestamp

Conditional payload fields:
- `applied_diff_text`: exact diff text applied (required on `success`)
- `error_code`: stable error code (required on `failed`)
- `error_detail`: diagnostic text (required on `failed`)

Rules:
- Apply event is emitted only after `decision=approved`.
- Apply execution is non-LLM patch application.
- Failed apply still must emit `self_improvement.applied` with `status=failed`.

## Why unified diff
- Text-only representation satisfies the requirement that proposal content be text.
- Deterministic patching can be implemented without LLM interpretation.
- Human review is straightforward in UI and logs.
- Existing engineering tooling and review practices are compatible with unified diff.

## Rejected alternatives
- Free-form replacement text without diff context:
  - Rejected because reviewers cannot reliably inspect scope and intent.
- Structured JSON operations for prompt proposals:
  - Rejected because prompt proposals are intentionally text-diff only.

## Notes for implementation
- Runtime should validate that `diff_text` is parseable unified diff for the selected target.
- If patch application fails, runtime emits `self_improvement.applied` with `status=failed` and error details.
- Concept-graph/memory auto-apply flows are separate from this prompt proposal schema.

## Compatibility Impact
breaking-by-default (no compatibility layer)
