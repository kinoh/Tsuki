---
date: 2026-03-10
---

# ADR: Visible History Limit Semantics

## Context

History assembly fetched the latest N raw events and then filtered debug/observability events
afterward. As debug output grew more verbose, `decision_history = 30` no longer meant the decision
model received 30 visible events.

## Decision

- `limits.decision_history` and `limits.submodule_history` mean the count of **visible**
  (non-debug, non-observability) events delivered to each context.
- History assembly continues fetching older events until the requested visible count is satisfied
  or the stream is exhausted.

## Rationale

Config names should reflect what the model actually sees. A visibility-based contract stays stable
when debug event volume changes.

## Compatibility Impact

breaking-by-default (no compatibility layer)
