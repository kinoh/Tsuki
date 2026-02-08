# Debug Batch Run for Submodules Node

## Context
- In debug UI, selecting the `Submodules` node previously only exposed submodule management controls.
- The desired behavior is to allow entering one input and running all active submodules in parallel without selecting a specific submodule.

## Decisions
- Keep the existing debug run endpoint shape and add a special module name path:
  - `POST /debug/modules/submodules/run`
- In backend routing (`debug_run_module`):
  - if `name == "submodules"`, execute all active submodules in parallel via `join_all`;
  - reuse `run_submodule_debug` for each module to keep event semantics identical.
- In debug UI:
  - when `Submodules` node is selected, show input and run controls;
  - keep submodule management controls visible in the same panel;
  - route `Run` to `moduleName=submodules`.

## Why
- This avoids adding a new endpoint and keeps the current debug API predictable.
- Reusing `run_submodule_debug` preserves existing worklog/raw event behavior.
- Parallel execution matches the runtime mental model of submodules better than serial manual runs.
