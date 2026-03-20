---
date: 2026-01-04
---

# ADR: gVisor Sandbox for MCP Shell Execution

## Context

Arbitrary shell commands needed to be executed via MCP tools. Launching MCP servers locally from
the core process lacked strong isolation.

## Decision

- Run a dedicated MCP server (`shell-exec`) inside a gVisor container (`runtime: runsc`).
- Connect from `core-rust` via streamable HTTP (`/mcp`).
- The MCP tool interface: `execute(command, args, stdin?, timeout_ms?)`.
- Outbound network access is allowed (required for web fetch scenarios).
- Output is byte-capped server-side; truncation is indicated explicitly.
- Only volume-backed paths (e.g. `/work`, `/memory`) are persisted.

## Sandbox Runtime Contract

The sandbox runtime may include practical helpers that materially improve tool reliability:
`python3` and `jq` are part of the runtime contract. This is an environment decision, not a
core-rust compatibility layer — the `execute` tool contract itself is unchanged.

## Rationale

gVisor provides kernel-level isolation without requiring a full VM. Streamable HTTP transport
allows containerized access. Direct argv execution (not shell-interpolated) avoids injection.

## Compatibility Impact

breaking-by-default (no compatibility layer)
