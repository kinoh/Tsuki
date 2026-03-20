---
date: 2026-03-02
---

# ADR: Server Boundary Split — Event and Module Runtime

## Context

`server_app` had accumulated both HTTP/WS transport responsibilities and event/module runtime
orchestration responsibilities. Feedback identified that event-stream processing and module-runtime
orchestration should not be owned by server transport code.

## Decision

- Event append, broadcast, and logging moved to `application/event_service.rs`.
- Module runtime definitions and initialization moved to `application/module_bootstrap.rs`
  (`Modules`, `ModuleRuntime`, `build_modules`, `sync_module_registry_from_prompts`).
- `server_app` delegates to application services and retains only transport-focused behavior.

## Rationale

Preserves separation of concerns:
- server layer: protocol/transport routing
- application layer: domain/runtime orchestration

Reduces coupling between HTTP/WS handling and core runtime behavior.

## Compatibility Impact

breaking-by-default (no compatibility layer)
