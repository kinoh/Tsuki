# Shell Exec MCP Server

## Overview
- Executes arbitrary shell commands inside the sandbox container.
- Exposes a single MCP tool that returns stdout/stderr/exit status as structured content.
- Uses streamable HTTP transport so the server can run in a container and be accessed over TCP.

## Configuration

### Environment Variables

- `MCP_EXEC_MAX_OUTPUT_BYTES` (optional): Maximum combined output size (stdout + stderr) in bytes. Defaults to `40000`.
- `MCP_EXEC_LOG_FULL_OUTPUT` (optional): When set to `1`, log full stdout/stderr up to the output limit.
- `MCP_EXEC_LOG_OUTPUT_BYTES` (optional): Number of bytes to log from stdout/stderr when full logging is disabled. Defaults to `2048`.
- `MCP_HTTP_BIND` (optional): Bind address for the HTTP server. Defaults to `0.0.0.0:8000`.
- `MCP_HTTP_PATH` (optional): HTTP path for MCP endpoint. Defaults to `/mcp`.

## Tools

### execute

Runs a command and returns the captured output.

#### Arguments

- `command` (required): Command string or executable name.
- `args` (optional): Array of arguments. When present, the command is executed directly without a shell.
- `stdin` (optional): String content passed to stdin.
- `timeout_ms` (optional): Timeout in milliseconds. The process is killed on timeout.

#### Behaviour

- If `args` is provided, the server executes `command` directly with the given arguments.
- If `args` is omitted, the server runs `sh -c <command>` to match typical shell input behavior.
- Stdout and stderr are captured and truncated to the configured maximum output size.
- Output is returned both as a JSON string and as structured content.

#### Errors

- `Error: command: empty` when `command` is empty or whitespace.
- `Error: execute: spawn failed` when the process cannot be started.
- `Error: execute: wait failed` when process termination cannot be observed.
- `Error: execute: read failed` when output streams cannot be read.

## Response Format

The tool returns JSON with the following fields:

```json
{
  "stdout": "stdout content",
  "stderr": "stderr content",
  "exit_code": 0,
  "timed_out": false,
  "stdout_truncated": false,
  "stderr_truncated": false,
  "elapsed_ms": 12
}
```

## Usage Patterns

### Direct argv execution

```json
{
  "tool": "execute",
  "arguments": {
    "command": "echo",
    "args": ["hello"]
  }
}
```

### Shell command string

```json
{
  "tool": "execute",
  "arguments": {
    "command": "perl -e \"print 1;\""
  }
}
```

## Implementation Notes

- Streamable HTTP transport is implemented via `rmcp::transport::streamable_http_server`.
- The server is designed to run inside the gVisor sandbox container.
