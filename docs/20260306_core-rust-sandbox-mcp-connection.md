# Core-Rust MCP Server Integration (`mcp_servers.*`)

## Overview
This document updates the integration decision for connecting `core-rust` to external MCP servers, including sandbox `shell-exec`, through a generic server registry in config.

Compatibility Impact: breaking-by-default (explicitly no backward compatibility).

## Problem Statement
`core-rust` currently has no external MCP client integration while deployment topology already provides MCP endpoints (for example, sandbox `shell-exec` over streamable HTTP).

A server-specific integration path (for only `shell-exec`) would create unnecessary coupling and does not scale to additional MCP servers.

## Responsibility Boundaries
- External MCP server behavior/tool semantics are owned by each MCP server implementation.
- `core-rust` owns:
  - MCP transport/session management
  - server registry loading from runtime config
  - tool discovery and invocation forwarding
  - explicit error surfacing in runtime events/tool outputs
- `core-rust` application orchestration decides when to call tools; transport layer must not embed domain-specific decision policy.

## Decision
- Adopt config-driven MCP server registry under `mcp_servers`:

```toml
[mcp_servers.shell_exec]
url = "http://localhost:8000/mcp"
```

- Add production overlay value in `config.prod.toml` (compose network hostname):

```toml
[mcp_servers.shell_exec]
url = "http://sandbox:8000/mcp"
```

- Implement generic MCP integration in `core-rust`; do not add `shell-exec`-specific adapter logic.
- Discover tools from connected servers and expose them through a generic forwarding layer.
- Enforce deterministic tool naming across servers to avoid collisions (for example, server-prefixed names).
- Do not always expose discovered MCP tools to LLM.
- Expose MCP tools only when their mapped concept activation reaches the soft recommendation threshold.
- When a mapped concept does not exist, create it idempotently (auto-create/upsert) before activation-based exposure.
- Build trigger-to-tool association edges at bootstrap:
  - trigger concept -> tool concept (`evokes`)
  - use natural language concepts directly (no synthetic trigger prefix namespace)
- Do not introduce compatibility fallback paths (no hidden env fallback, no dual transport path).

## Why
- The requested config shape (`mcp_servers.<name>`) matches common coding-agent MCP conventions and is naturally extensible.
- Generic integration avoids per-server hardcoding and keeps responsibility boundaries clean.
- `shell-exec` does not require custom client logic in `core-rust` as long as MCP call forwarding is implemented correctly.
- Explicit error surfacing with per-server failure isolation prevents silent degradation while keeping unaffected MCP servers available.

## Contract and Naming Policy
- Runtime config contract:
  - `mcp_servers` is a map keyed by server id.
  - each entry currently requires `url`.
- Tool exposure contract:
  - tools are discovered from MCP `tools/list`.
  - runtime tool names must be collision-safe and stable.
  - recommended rule: `<server_id>__<tool_name>`.
  - discovered tools are `available`; each turn only a subset becomes `visible` to LLM.
  - visibility is activation-gated, not static.
- Invocation contract:
  - runtime forwards arguments as-is to MCP `tools/call`.
  - no server-specific argument rewriting in `core-rust`.
- Tool-to-concept contract:
  - each MCP tool must deterministically resolve to one concept key.
  - if the concept is missing, runtime must auto-create the concept idempotently.
  - if deterministic mapping cannot be built (for example invalid normalized key or collision), mark the tool unavailable and log a concrete error.
- Trigger concept onboarding contract:
  - trigger concepts are generated from MCP tool definitions by LLM at bootstrap.
  - generated trigger concepts must be stored as normal concept names (no `mcp_trigger:*` namespace split).
  - runtime must create `evokes` edges in direction `trigger_concept -> tool_concept`.
  - onboarding is successful only if at least one trigger edge is created for the tool.
  - onboarding failure is isolated to the target tool; unrelated tools/servers continue.
- Trigger generation validation contract (minimum required checks only):
  - parse check: LLM output must parse into JSON object with `trigger_concepts: string[]`.
  - non-empty check: after trim and exact dedupe, at least one trigger concept remains.
  - edge check: at least one `relation_add(trigger_concept, tool_concept, \"evokes\")` succeeds.

## Failure Policy
- Startup:
  - invalid `mcp_servers` entry format is a config error.
  - connection/bootstrap failures are isolated per server; one failed server must not disable other MCP servers.
- Invocation:
  - MCP transport/protocol errors must surface as explicit tool errors.
  - no silent swallow and no implicit retries unless explicitly designed later.
- Mapping failures:
  - mapping failures are handled through the same error pipeline as other tool/runtime errors.
  - mapping failures must not propagate to unrelated servers/tools.
- Observability:
  - tool observation events must contain concrete failure cause and server id/tool name.
  - bootstrap onboarding details (auto-create, mapping failure, trigger-edge build result) are reported via logs, not dedicated event types.
  - tool visibility (`visible`/`hidden`) is turn-scoped and must be attached to router turn events with reason.

## Threshold Policy
- MCP tool visibility threshold is the same value as router soft recommendation threshold.
- Do not introduce a separate MCP-only threshold at this stage.

## Implementation Scope
Planned code paths:
- `core-rust/src/config.rs`
  - add `mcp_servers` config map types.
- `core-rust/config.toml` and `core-rust/config.prod.toml`
  - define `mcp_servers.shell_exec.url` values.
- `core-rust/src/mcp/*` (new generic integration layer)
  - connect servers from config
  - list tools
  - call tools via generic forwarding
- `core-rust/src/application/pipeline_service.rs` and router event payloads
  - compute turn-level MCP tool visibility from concept activation
  - attach visibility decisions to router events
- `core-rust/src/application/module_bootstrap.rs`
  - pass only turn-visible MCP tools into module runtime tool list.
- `core-rust/src/tools.rs` (or dedicated forwarding module)
  - resolve runtime tool name -> (server id, remote tool name)
  - forward call to connected MCP client.
- `core-rust/src/server_app.rs`
  - initialize MCP integration at startup
  - expose available (discovered) tools in metadata.
  - do not expose turn-level visibility in metadata.

## Out of Scope
- `shell-exec` server implementation changes.
- Server-specific policy overlays (allowlist/denylist/forced timeout) at this stage.
- Per-user MCP tenancy redesign.

## Future Considerations
- Add integration tests for multi-server registration and tool-name collision handling.
- Add health/status endpoint for MCP connection state per server.
- If server-specific guardrails become required, add them as explicit policy modules (not ad-hoc branching in transport code).
