---
date: 2026-02-22
---

# ADR: Self-Improvement Proposal Schema (Unified Diff)

## Context

The self-improvement flow requires auditable proposal, review, and apply steps. Apply must be
deterministic and non-LLM. Proposal format must be reviewable by humans in logs and UI.

## Decision

Prompt improvement proposals use **unified diff** as the sole content format.

Key payload fields:
- `diff_text` — unified diff against the current prompt text of the target.
- `target` — one of: `base`, `router`, `decision`, `submodule:<name>`.
- `requires_approval` — always `true` for prompt improvements.

Event sequence:
1. `self_improvement.proposed` — carries `diff_text`, `target`, `proposal_id`, `job_id`.
2. `self_improvement.reviewed` — carries `decision: approved|rejected`, `review_reason`.
3. `self_improvement.applied` — carries `status: success|failed`; on success includes
   `applied_diff_text`; on failure includes `error_code` and `error_detail`.

Apply is triggered only after `decision=approved`. Failed apply still emits the applied event with
`status=failed`.

## Rationale

Unified diff is text-only, human-reviewable, and patched deterministically. Structured JSON
operations for prompt proposals are rejected because prompt proposals are intentionally text-diff
only. Free-form replacement text is rejected because reviewers cannot reliably inspect scope and
intent.

## Compatibility Impact

breaking-by-default (no compatibility layer)
