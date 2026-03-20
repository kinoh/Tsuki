# Debug API Contract Module

## Overview
This document records the extraction of shared debug API request and response types out of `server_app.rs`.

Compatibility Impact: Internal only. HTTP endpoints and payload shapes are unchanged.

## Problem Statement
`server_app.rs` owned request and response DTOs that were also consumed by `application/*`.
That made the application layer depend on server-owned types even when the types themselves were generic debug contracts rather than router wiring details.

## Solution
Move the shared debug request and response types into `core-rust/src/debug_api.rs` and re-export them from the crate root.

Extracted types:

- `DebugRunRequest`
- `DebugRunResponse`
- `DebugTriggerRequest`
- `DebugTriggerResponse`
- `DebugImproveProposalRequest`
- `DebugImproveReviewRequest`
- `DebugImproveResponse`

## Design Decisions
### Keep the module small and contract-focused
The new module only contains payload types.
It does not take on routing logic or application logic.

### Preserve current crate-root imports
Existing application modules still import the DTOs through the crate root.
This keeps the refactor narrow while removing server ownership of the contract types.

## Future Considerations
- If more boundary types accumulate, this module may need to split by feature area instead of staying as one shared `debug_api` file.
- The next responsibility cleanup step should remove remaining `application/* -> server_app` coupling around route-owned orchestration and shared state access.
