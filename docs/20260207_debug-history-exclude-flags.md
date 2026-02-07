# Debug History Exclude Flags

## Context
- Debug reruns sometimes need partial rollback without deleting events.
- Deleting events is risky and increases implementation scope.

## Decision
- Add per-event exclude flags in Debug UI Work Log.
- Use a small `x` button on each log row to toggle exclusion.
- Send `exclude_event_ids` with debug run requests.

## Why
- Keeps implementation small and reversible.
- Preserves full event logs while allowing selective history omission.
- Supports iterative "rerun from here" workflows without destructive operations.

## Implementation Notes
- Backend: `DebugRunRequest` accepts optional `exclude_event_ids: Vec<String>`.
- Backend history builder filters out events whose `event_id` is in the exclusion set.
- UI: excluded rows are visually dimmed; toggling is idempotent.
- UI regression fix: `exclude` button is rendered as plain `x` (no border/background), and Work Log reserves scrollbar gutter + right padding to avoid overlap with items.
