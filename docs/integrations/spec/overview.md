# Integrations — Overview

Integrations are external services and MCP servers that extend the core's capabilities. The core
connects to them as a client; it does not own their tool descriptions or schemas.

## MCP Servers

MCP servers provide tools to the core via the Model Context Protocol. The core discovers tools at
bootstrap and exposes them to Decision only when their mapped concept reaches the soft activation
threshold (activation-gated visibility).

| Server | Role |
|---|---|
| `shell-exec` | Shell command execution inside the gVisor sandbox |
| `rss-mcp-lite` | Shared RSS feed access |

Tool naming convention: `<server_id>__<tool_name>` (collision-safe, stable).

## Sandbox

`shell-exec` runs inside a gVisor container (`runtime: runsc`). Commands execute via direct argv
(no shell interpolation). Output is byte-capped. The sandbox runtime includes `python3` and `jq`
as part of its capability contract.

## Skill Packages

Skills are installed into the sandbox filesystem and indexed in the concept graph. The sandbox
owns skill content; the concept graph owns metadata and retrieval. See `core/spec/skill-model.md`.

## Key Constraints

- Tool descriptions and schemas are owned by each MCP server — core must not rewrite them.
- Initial trigger concepts generated at bootstrap must be generic action-category concepts, not
  scenario-specific use cases.
- MCP server connection failures are isolated per server; one failed server must not disable
  others.
