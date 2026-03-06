# Explicit Module Imports

## Overview
This document records the removal of crate-root re-exports that had been masking the real ownership of shared runtime types.

Compatibility Impact: Internal only. Runtime behavior and external contracts are unchanged.

## Problem Statement
`main.rs` re-exported `AppState`, `record_event`, runtime bootstrap types, and debug DTOs.
That let internal modules import everything from `crate::{...}`, but it also hid where those dependencies were actually owned.

This made dependency review harder because code appeared to depend on the crate root instead of the concrete module that defined the contract.

## Solution
Update internal modules to import dependencies from their owning modules directly:

- `AppState` from `app_state`
- `record_event` from `application::event_service`
- `ModuleRuntime` and `Modules` from `application::module_bootstrap`
- debug request/response DTOs from `debug_api`

After that, remove the crate-root re-exports from `main.rs`.

## Design Decisions
### Prefer explicit ownership over convenience re-exports
The code becomes slightly more verbose, but each import now reveals the real responsibility boundary.
That is more valuable than shorter import lists during refactor work.

### Keep the crate root minimal
`main.rs` should define the executable entrypoint and top-level modules.
It should not act as a shared facade for unrelated internal contracts.

## Future Considerations
- Continue reducing hidden dependencies by moving remaining shared contracts to focused modules instead of routing them through broad files.
