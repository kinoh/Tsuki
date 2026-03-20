---
date: 2026-03-05
---

# ADR: `/events` Tags Filter — OR Semantics, Debug Off by Default

## Context

The GUI fetched raw events and discarded non-display events client-side. When debug events were
dense, `limit=20` often returned far fewer than 20 visible messages.

## Decision

- `GET /events` accepts a `tags` query parameter (repeatable: `tags=input&tags=response`).
- Matching is OR: an event is included when it has at least one requested tag.
- When `tags` is omitted, events with the `debug` tag are excluded by default.
- When `tags` is provided, `debug` events are still excluded unless `debug` is explicitly included.
- Server-side batched scan fills `limit` after filtering (batch size: `clamp(limit*4, 50, 500)`;
  scan cap: 5000 events per request).

## Rejected Alternatives

- Include/exclude dual parameter: rejected (user requirement explicitly rejected this).
- `text_only` filter: rejected (breaks multimodal extensibility).
- AND semantics: rejected (GUI needs `response OR input`, not intersection).

## Rationale

OR semantics match the GUI use case. Debug-off by default prevents verbose debug payloads from
appearing unless explicitly requested. Server-side filtering ensures the requested `limit` reflects
visible events, not raw fetched count.

## Compatibility Impact

Additive for existing clients without `tags` (same behavior). breaking-by-default policy otherwise
preserved.
