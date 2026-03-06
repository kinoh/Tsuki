# AppState Construction

## Overview
This document records the shift of `AppState` construction responsibility toward `app_state.rs`.

Compatibility Impact: Internal only. No API contract changes.

## Problem Statement
Even after moving the `AppState` type out of `server_app.rs`, the server layer still assembled every nested state object inline.
That left ownership split awkwardly between:

- type definitions in `app_state.rs`
- construction details in `server_app.rs`

## Solution
Add small constructor helpers in `app_state.rs` and use them from `server_app.rs`:

- `AppState::new`
- `AuthState::new`
- `PromptState::new`
- `ResolvedPrompts::new`
- `RuntimeState::new`

This keeps `server_app.rs` responsible for bootstrapping dependencies while `app_state.rs` owns how grouped runtime state is assembled.

## Design Decisions
### Use small constructors instead of a builder pattern
The state shape is still evolving quickly.
A full builder would add ceremony without improving clarity at this stage.

### Keep grouping decisions close to the grouped types
Once `AuthState`, `PromptState`, and `RuntimeState` exist, their creation rules should live next to those types.
That makes future adjustments to the grouped state less likely to leak back into server routing code.

## Future Considerations
- If bootstrapping logic keeps growing, the next step should move state assembly into a dedicated bootstrap module rather than expanding `server_app.rs`.
