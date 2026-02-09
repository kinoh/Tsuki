# Event Log Redefinition and `debug/worklog` Role

## Context
- Current debug UI treats `Work Log` as a view backed by `debug,worklog` plus `action,response`.
- This requires explicit `debug,worklog` emission whenever a newly visible event kind is introduced.
- User feedback identified this as a scaling problem and a source of ambiguity for context control.

## Decision
- Redefine `Work Log` as **Event Log**.
- Event Log is the primary UI surface for runtime context control (`exclude_event_ids`, `history_cutoff_ts`).
- Event Log should show all persisted events by default, with practical display grouping for readability.

## Why
- Prevents per-event-kind maintenance on `debug,worklog` emission.
- Keeps context control aligned with the real event stream consumed by history formatting.
- Separates production semantics from debug observability without losing either.

## Scope Clarification
- "Show all events" means all persisted events are queryable and visible in Event Log.
- For usability, large debug payloads (for example `debug,llm.raw`) may be collapsed or shown in a dedicated detail section.
- This is a presentation concern only; it must not change event persistence semantics.

## `debug/worklog` Position
- `debug/worklog` is not the source of truth for context.
- It remains optional as a derived debug artifact for paired inspection (input/output snapshots, run mode breadcrumbs).
- If retained, it must not be required to make a primary event visible or controllable.

## Context Control Semantics
- `exclude_event_ids` targets primary event ids selected in Event Log.
- `history_cutoff_ts` targets a timestamp anchor selected from Event Log.
- These controls define what enters `Recent event history` for module/decision runs.
- Mapping through synthetic debug-only ids should be avoided.

## Data/UX Model
- Event Log row model:
  - `event_id`
  - `ts`
  - `source`
  - `tags`
  - summary payload text
- Suggested default filters/toggles:
  - `all` (default)
  - `non-debug`
  - `debug-only`
  - module/tag quick filters
- Raw payload inspection should remain available from each row.

## Migration Direction
1. Event Log fetch path should include all events (or a superset-first query), not only `debug,worklog`.
2. Selection/exclude/cutoff state should store primary event identifiers directly.
3. Existing `debug,worklog` rendering can remain during transition, but as secondary metadata.
4. Final state removes any requirement that adding a new primary event kind must also add `debug,worklog` emission.

## Explicit Notes from User Feedback
- Event Log should be principle-first: all events are visible.
- Separation between production-consumable outputs and debug inspection must be maintained.
- Prior wording around sending "selected events as-is" was ambiguous; this document clarifies that controls are bound to primary events shown in Event Log.

## Non-Goals
- No immediate deletion of existing debug tags/data.
- No change to event storage schema in this decision document.

