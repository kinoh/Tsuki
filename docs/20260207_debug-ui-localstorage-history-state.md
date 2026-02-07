# Debug UI Local Persistence for Cutoff/Exclusions

## Context
- Debug rerun setup relies on two UI-only states:
  - history cutoff timestamp
  - excluded event IDs
- These states were reset on page reload, causing repeated manual setup.

## Decision
- Persist cutoff and excluded event IDs in browser `localStorage`.
- Scope is debug UI only and does not require backend changes.

## Why
- Keeps implementation minimal and reversible.
- Matches developer workflow where page reloads are common during tuning.
- Avoids introducing new storage APIs for temporary debug controls.

## Implementation Notes
- Storage key: `tsuki_core_rust_debug_ui_state_v1`.
- Saved fields:
  - `historyCutoffTs`
  - `excludedEventIds`
  - `appendInputMode`
- Load on page init, save on:
  - exclusion toggle
  - cutoff set/clear
  - post-refresh cleanup when stale IDs are removed.
