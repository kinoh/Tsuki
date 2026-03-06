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
- Keep fail-fast startup behavior for invalid server config and connection/bootstrap errors.
- Do not introduce compatibility fallback paths (no hidden env fallback, no dual transport path).

## Why
- The requested config shape (`mcp_servers.<name>`) matches common coding-agent MCP conventions and is naturally extensible.
- Generic integration avoids per-server hardcoding and keeps responsibility boundaries clean.
- `shell-exec` does not require custom client logic in `core-rust` as long as MCP call forwarding is implemented correctly.
- Fail-fast startup prevents silent partial runtime where tools appear configured but are unavailable.

## Contract and Naming Policy
- Runtime config contract:
  - `mcp_servers` is a map keyed by server id.
  - each entry currently requires `url`.
- Tool exposure contract:
  - tools are discovered from MCP `tools/list`.
  - runtime tool names must be collision-safe and stable.
  - recommended rule: `<server_id>__<tool_name>`.
- Invocation contract:
  - runtime forwards arguments as-is to MCP `tools/call`.
  - no server-specific argument rewriting in `core-rust`.

## Failure Policy
- Startup:
  - invalid URL or failed MCP bootstrap must fail startup.
- Invocation:
  - MCP transport/protocol errors must surface as explicit tool errors.
  - no silent swallow and no implicit retries unless explicitly designed later.
- Observability:
  - tool observation events must contain concrete failure cause and server id/tool name.

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
- `core-rust/src/application/module_bootstrap.rs`
  - include discovered MCP tools in module runtime tool list.
- `core-rust/src/tools.rs` (or dedicated forwarding module)
  - resolve runtime tool name -> (server id, remote tool name)
  - forward call to connected MCP client.
- `core-rust/src/server_app.rs`
  - initialize MCP integration at startup
  - expose connected/discovered tools in metadata.

## Out of Scope
- `shell-exec` server implementation changes.
- Server-specific policy overlays (allowlist/denylist/forced timeout) at this stage.
- Per-user MCP tenancy redesign.

## Future Considerations
- Add integration tests for multi-server registration and tool-name collision handling.
- Add health/status endpoint for MCP connection state per server.
- If server-specific guardrails become required, add them as explicit policy modules (not ad-hoc branching in transport code).
