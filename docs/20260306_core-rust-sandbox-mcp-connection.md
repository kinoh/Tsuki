# Core-Rust Sandbox MCP Connection

## Overview
This document defines the design decision to connect `core-rust` to the sandbox MCP endpoint (`shell-exec`) over streamable HTTP.

Compatibility Impact: breaking-by-default (explicitly no backward compatibility).

## Problem Statement
`core-rust` currently has no MCP client integration for sandbox command execution:
- `sandbox` service exists in compose and exposes MCP HTTP at `/mcp`.
- `core-rust` runtime config has no sandbox MCP endpoint section.
- `core-rust` metadata currently reports `mcp_tools: []`.

As a result, `core-rust` cannot invoke sandbox tools even though the sandbox server is available in deployment topology.

## Responsibility Boundaries
- Sandbox process execution responsibility remains in `mcp/shell-exec`.
- `core-rust` owns transport-level MCP client integration and tool invocation policy.
- Application orchestration decides when to call sandbox tool(s); transport code must not embed business-level policies.
- Event persistence/logging of tool outcomes remains owned by `core-rust` runtime event model.

## Decision
- Add an explicit non-secret runtime config section for sandbox MCP in `core-rust/config.toml`.
- Integrate MCP client support in `core-rust` and connect to sandbox via streamable HTTP (`http://sandbox:8000/mcp` in compose runtime).
- Expose sandbox execution as an explicit tool in module runtime (e.g., `sandbox_execute`), mapped to the sandbox MCP `execute` tool.
- Keep fail-fast startup behavior when sandbox MCP config is missing/invalid if sandbox integration is enabled.
- Do not introduce compatibility fallbacks (no hidden env fallback, no dual path execution).

## Why
- Current topology already provides a dedicated gVisor-isolated sandbox service; not using it in `core-rust` leaves execution capability disconnected.
- MCP-native integration preserves protocol consistency and future extensibility better than ad-hoc HTTP wrappers.
- Explicit config ownership aligns with the current `core-rust` policy: non-secret runtime settings in `config.toml`, secrets in env.
- Fail-fast behavior prevents silent partial runtime where tools appear available conceptually but are not callable.

## Contract and Failure Policy
- Input contract for sandbox execution tool:
  - `command: string` (required)
  - `args: string[]` (optional)
  - `stdin: string` (optional)
  - `timeout_ms: integer` (optional)
- Output contract mirrors MCP `shell-exec` response payload (`stdout`, `stderr`, `exit_code`, `timed_out`, truncation flags, `elapsed_ms`).
- Runtime behavior on errors:
  - Tool invocation errors must be surfaced as explicit tool error outputs.
  - No silent swallow, no implicit retry policy.
  - Tool observation events must include concrete failure reason.

## Implementation Scope
Planned code paths:
- `core-rust/src/config.rs`
  - add sandbox MCP config section.
- `core-rust/config.toml` and `core-rust/config.prod.toml`
  - add sandbox MCP endpoint values.
- `core-rust/src/*` MCP integration layer
  - initialize MCP client and bind sandbox tool call.
- `core-rust/src/application/module_bootstrap.rs`
  - register sandbox tool in module runtime tool list.
- `core-rust/src/tools.rs` (or dedicated adapter file)
  - add adapter that maps runtime tool call to MCP `execute`.
- `core-rust/src/server_app.rs`
  - include connected MCP tool names in metadata.

## Out of Scope
- Command allowlist/denylist policy changes.
- Sandbox server implementation changes (`mcp/shell-exec`).
- Multi-sandbox routing or per-user sandbox tenancy redesign.

## Future Considerations
- Add integration tests for sandbox MCP connection health and tool round-trip behavior.
- Add explicit monitoring metric for sandbox tool call success/error rate.
- Consider policy-layer restrictions after baseline connection is stable.
