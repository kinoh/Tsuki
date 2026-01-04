# MCP Shell Command Sandbox (gVisor)

## Overview
Provide an MCP server that executes arbitrary shell commands inside a gVisor-isolated container. The MCP tool exposes a simple arg/result interface while enforcing lightweight output limits.

## Problem Statement
We need to run arbitrary shell commands via MCP tools. Existing MCP servers are launched locally by the core process, which lacks strong isolation. A gVisor sandbox already exists in compose, but no MCP server uses it.

## Solution
Run a dedicated MCP server inside a gVisor container and connect to it from core via a bridge endpoint. The MCP server accepts a command and arguments, executes them, and returns stdout/stderr/exit status while enforcing output limits.

## Design Decisions
- **Isolation boundary**: Execute commands only inside the gVisor container (`runtime: runsc`), not on the host.
- **Network**: Allow outbound network access as required by the use case.
- **Limits**:
  - CPU/memory enforced via compose limits.
  - No input size limit at the MCP layer.
  - Output size capped by byte count (approximate 10K token cap; no expensive tokenization).
- **Persistence**: Only paths backed by Docker volumes are persisted; the container itself is not read-only.
- **Interface**: Provide a single MCP tool (`execute`) with `command`, `args`, optional `stdin`, and `timeout_ms`.

## Implementation Details
- **Compose service**:
  - `code-sandbox` service with `runtime: runsc`.
  - Run the MCP server inside the container (e.g., `./bin/code-exec`).
  - Use a dedicated volume for working directory (e.g., `sandbox-work:/work`).
  - Allow writes in the container, but persist only volume-backed paths such as `/work`.
- **MCP server behavior**:
  - Execute via direct argv (prefer direct argv to avoid shell injection).
  - Enforce output limit by truncating stdout/stderr to N bytes and indicating truncation.
  - Enforce timeout via process kill.
- **Core integration**:
  - Add a new MCP server entry that connects to the sandbox MCP endpoint.
  - Configure max output size via env (e.g., `MCP_EXEC_MAX_OUTPUT_BYTES`).

## Future Considerations
- Optional per-request temp directories with cleanup.
- Audit logging of executed commands.
- Optional allowlist/denylist of commands for safety.
