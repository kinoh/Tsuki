# Debug UI History Cutoff in Work Log

## Context
- History inclusion for debug runs was controlled by a right-panel checkbox.
- This made history scope hard to reason about from the event timeline.

## Decision
- Move history scope control to the left Work Log panel.
- Use `Set cutoff` / `Clear cutoff` controls next to `Refresh` in Work Log.
- Keep Work Log click as row selection only.
- Show cutoff by a divider line in the log list.
  - No cutoff set: divider is at the very top (above all events).
  - Cutoff set: divider is attached to the selected cutoff event row.

## Why
- The timeline itself is the source of truth for history boundaries.
- A divider marker keeps context visible while reducing visual noise.
- This matches debugging workflows where users want reproducible context windows.

## Implementation Notes
- UI sends `include_history=false` when no cutoff is set (top divider).
- UI sends `include_history=true` and `history_cutoff_ts` when cutoff is set.
- Backend filters history events to `event.ts >= history_cutoff_ts`.
- In the Work Log, this means events newer than the divider are included and older ones are excluded.
- Right-panel `Include history` toggle is removed in favor of Work Log cutoff control.
