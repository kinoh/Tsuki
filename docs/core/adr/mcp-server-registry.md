---
date: 2026-03-06
---

# ADR: Config-Driven MCP Server Registry

## Context

`core-rust` needed external MCP client integration (starting with sandbox `shell-exec`). A
server-specific integration path would create unnecessary coupling and not scale to additional
servers.

## Decision

- MCP servers are registered under `[mcp_servers.<id>]` in `config.toml` with a `url` field.
- `core-rust` owns MCP transport/session management, tool discovery, and invocation forwarding.
- Tool descriptions and schemas are owned by each MCP server — `core-rust` must not rewrite them.
- Discovered tools are **activation-gated**: a tool becomes visible to Decision only when its
  mapped concept activation reaches the soft recommendation threshold. Visibility is turn-scoped.
- Each MCP tool maps deterministically to one concept key. If the concept is missing, it is
  created idempotently at bootstrap.
- Bootstrap trigger generation uses LLM to build `evokes` edges from trigger concepts to tool
  concepts. Onboarding failure is isolated per tool; other tools/servers continue.

## Tool Naming Contract

Runtime tool names must be collision-safe and stable: `<server_id>__<tool_name>`.

## Failure Policy

- Connection/bootstrap failures are isolated per server.
- MCP transport errors surface as explicit tool errors — no silent swallow.
- Mapping failures do not propagate to unrelated servers or tools.

## Rationale

Generic integration avoids per-server hardcoding. Activation-gated visibility keeps MCP tools
from polluting Decision context when they are not relevant to the current turn.

## Compatibility Impact

breaking-by-default (no compatibility layer)
