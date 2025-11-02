# Metrics MCP server for Prometheus-compatible backends

## Overview
- Fetches point-in-time metric snapshots from a Prometheus HTTP API (backed by VictoriaMetrics in production)
- Uses a pre-defined query catalogue so agents can only request approved metrics
- Integrates with Tsuki's MCP launcher alongside `structured-memory`, `scheduler`, and `weather`

## Configuration

### Environment Variables

- `PROMETHEUS_BASE_URL` (required): Base URL of the Prometheus-compatible endpoint, e.g. `https://victoria.example.com`.
- `METRICS_CONFIG_PATH` (required): Absolute path to a JSON or YAML file describing the allowed metric queries.
- `DEFAULT_STEP` (optional): Step size for range queries in seconds when VictoriaMetrics requires it. Defaults to `60`.

### Metric Catalogue File

The config file maps exported metric IDs to PromQL expressions and optional descriptions:

```json
{
  "core_requests_per_minute": {
    "query": "sum(rate(tsuki_core_requests_total[5m]))",
    "description": "Rolling 5 minute request rate across all channels"
  },
  "core_memory_usage": {
    "query": "max(tsuki_core_process_resident_memory_bytes)"
  }
}
```

This catalogue is loaded at startup; agents cannot supply ad-hoc queries.

## Tools

### get_metric

Retrieves one or more configured metric snapshots.

#### Arguments

- `id` (optional): Metric ID from the catalogue. When omitted, all configured metrics are returned.
- `at` (optional): RFC3339 timestamp (`2024-09-18T03:15:00Z`). Uses the latest value if omitted.

#### Behaviour

- Uses the Prometheus `/api/v1/query` endpoint with the configured expression.
- When `at` is supplied, the query is executed with the `time` parameter to obtain the historical point-in-time value.
- When `at` is omitted, the server queries the latest sample and returns the freshest value VictoriaMetrics exposes.
- Results include the numeric value, the query expression, and the scrape timestamp returned by VictoriaMetrics.

#### Errors

- `Error: id: unknown metric` when `id` is not present in the catalogue.
- `Error: at: invalid timestamp` when the timestamp cannot be parsed.
- `Error: upstream: ...` when the Prometheus API returns an error payload or non-200 status.

## Usage Patterns

### Fetch the latest request rate

```json
{
  "tool": "get_metric",
  "arguments": {
    "id": "core_requests_per_minute"
  }
}
```

### Fetch all core health metrics at a specific time

```json
{
  "tool": "get_metric",
  "arguments": {
    "at": "2024-09-18T03:15:00Z"
  }
}
```

## Implementation Notes

- Follow the Rust server conventions established in `mcp/structured-memory` and `mcp/scheduler` (Tokio runtime, tool registry, MCP transport).
- Leverage `reqwest` for HTTPS requests and reuse the shared MCP logging/telemetry helpers when available.
- VictoriaMetrics is API-compatible with Prometheus, so standard `/api/v1/query` semantics apply.
