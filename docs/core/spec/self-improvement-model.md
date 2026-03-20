# Self-Improvement Model

## Design Principle

Self-improvement is an auditable, human-in-the-loop flow. Every proposal must be reviewable before
it changes runtime behavior. Apply is deterministic and non-LLM.

## Event Flow

```
self_improvement.run (trigger)
  → self_improvement.proposed   (diff_text, target, requires_approval=true)
  → self_improvement.reviewed   (decision: approved|rejected)
  → self_improvement.applied    (status: success|failed)
```

Each step is an event in the stream. The flow does not short-circuit: a failed apply still emits
`self_improvement.applied` with `status=failed`.

## Prompt Targets

`target` is one of: `base`, `router`, `decision`, `submodule:<name>`.

Memory (`## Memory` section) can only be proposed for `target=decision`. All other targets accept
structural prompt edits (`proposal.target`) but not memory section updates.

## Prompt Source

All prompt text, including self-improvement worker instructions (`# Self Improvement` section),
comes from `prompts.md`. There is no hardcoded fallback. Runtime fails fast at startup if any
required section is missing.

## Extension Guidance

- The proposal/review/apply event triad is the extension point for new improvement kinds.
- Do not add a new improvement flow that skips the review step unless approval semantics are
  explicitly redesigned with a dedicated decision record.
- Auto-apply (memory writes without human review) is a distinct flow from prompt diff proposals
  and must be documented separately if introduced.
