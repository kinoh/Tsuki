---
date: 2026-03-14
---

# ADR: Decision Context — Recent History as Primary

## Context

The previous decision context template placed `recent_event_history` and `recalled_event_history`
as similarly shaped blocks without explicit authority boundaries. The layout made it unclear which
source was authoritative for conversational flow coherence.

## Decision

- `recent_event_history` is the **first and primary** block in the decision context.
- All other sections are nested under a single `<supplemental_context>` block.
- The template uses XML-style tags (not ad-hoc bracket markers).
- `recalled_event_history` carries an explicit constraint: it must only be used when consistent
  with `recent_event_history`.
- No synthetic priority attribute layer is introduced.

## Rationale

Recent visible history is the canonical signal for conversational flow. `recalled_event_history`
is useful context but must not outrank the ongoing conversation. XML tags make boundaries legible
to both operators and the model.

## Rejected Alternatives

- Flat section labels only: leaves primary and supplemental context too close in authority.
- `priority=*` attributes: adds abstraction without clarifying authority as well as explicit
  primary/supplemental structure.

## Compatibility Impact

breaking-by-default (no compatibility layer)
