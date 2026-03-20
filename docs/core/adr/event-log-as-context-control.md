---
date: 2026-02-09
---

# ADR: Event Log as Context Control Surface

## Context

The debug UI treated `Work Log` as a derived view backed by `debug,worklog` events. This required
explicit `debug,worklog` emission whenever a new event kind was introduced, and conflated debug
observability with runtime context control.

## Decision

- The primary UI surface for runtime context control is the **Event Log** — all persisted events,
  queryable by default.
- Context control primitives (`exclude_event_ids`, `history_cutoff_ts`) operate on primary event
  ids selected from the Event Log.
- `debug,worklog` is not the source of truth for context. It may be retained as an optional derived
  artifact for paired inspection but must not be required for an event to be visible or
  controllable.
- Large debug payloads (e.g. `debug,llm.raw`) may be collapsed in the UI; this is a presentation
  concern only and must not change event persistence semantics.

## Rationale

Prevents per-event-kind maintenance overhead on `debug,worklog` emission. Keeps context control
aligned with the real event stream consumed by history formatting. Separates production semantics
from debug observability.

## Compatibility Impact

breaking-by-default (no compatibility layer)
