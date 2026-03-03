# Server Boundary Split: Event and Module Runtime

## Context
- Feedback: event-stream processing and module-runtime orchestration should not be owned by server transport code.
- `server_app` had both HTTP/WS transport and event/module runtime responsibilities.

## Decision
- Moved event append/broadcast/logging to `application/event_service.rs`.
- Moved module runtime definitions and initialization to `application/module_bootstrap.rs`:
  - `Modules`
  - `ModuleRuntime`
  - `build_modules`
  - `sync_module_registry_from_prompts`
- `server_app` now delegates to application services and keeps transport-focused behavior.

## Why
- Preserves separation of concerns:
  - server layer: protocol/transport routing
  - application layer: domain/runtime orchestration
- Reduces coupling between HTTP/WS handling and core runtime behavior.
