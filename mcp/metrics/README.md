# Metrics MCP server for Prometheus-compatible backends

## Overview
- Fetches point-in-time metric snapshots from a Prometheus HTTP API (VictoriaMetrics backend in production)
- Loads a fixed catalogue of PromQL queries from a multi-line environment variable so deployment stays self-contained
- Exposes a single MCP tool that returns all configured metrics in TOON format

## Configuration

### Environment Variables

- `PROMETHEUS_BASE_URL` (required): Base URL of the Prometheus-compatible endpoint, e.g. `https://victoria.example.com`.
- `METRICS_QUERIES` (required): Multi-line string describing the queries to execute. Each line uses `name=query` format.
  - Example:
    ```bash
    export METRICS_QUERIES=$'temperature=avg(tsuki_temperature_celsius)\nrequests=sum(rate(tsuki_core_requests_total[5m]))'
    ```
  - Names serve as aliases when Prometheus results lack a `__name__` label. Queries must not contain newlines.
- `TZ` (required): Timezone which response uses.
- `PROMETHEUS_BASIC_AUTH_USERNAME` / `PROMETHEUS_BASIC_AUTH_PASSWORD` (optional, pair): When both are set, requests include HTTP Basic Auth credentials. Leave unset for unauthenticated endpoints.

## Query Catalogue Semantics

- Queries are loaded at startup; agents cannot provide ad-hoc expressions.
- All configured queries are evaluated on every tool invocation.
- A query line without `=` or with an empty expression is ignored and reported in logs during startup.

## Tools

### get_metrics

Retrieves the configured metric snapshots.

#### Arguments

- `at` (optional): RFC3339 timestamp (`2024-09-18T03:15:00Z`). Uses the latest value if omitted.

#### Behaviour

- Issues one `/api/v1/query` request per configured query using the provided PromQL expression.
- When `at` is supplied, the request includes the `time` parameter to obtain historical values.
- When `at` is omitted, the latest sample available in VictoriaMetrics is returned.
- Every query result is summarised into TOON format (see below) and aggregated into a single response string.

#### Errors

- `Error: at: invalid timestamp` when the timestamp cannot be parsed.
- `Error: upstream: ...` when the Prometheus API responds with an error payload or non-200 status.
- `Error: metrics: not configured` when no valid queries were loaded at startup.

## Response Format (TOON)

Results are returned using [TOON](https://github.com/johannschopplich/toon) so downstream agents can parse a compact tabular form:

```
results[2]{name,timestamp,value}:
  temperature,2025-11-01T20:00:00+09:00,23.5
  requests,2025-11-01T20:00:00+09:00,125
```

- `name`: `__name__` label from the Prometheus sample (falls back to the configured alias when missing).
- `timestamp`: RFC 3339 local time.
- `value`: Parsed numeric value from the Prometheus sample (NaN/Inf converted to strings).

## Usage Patterns

### Fetch latest metrics

```json
{
  "tool": "get_metrics",
  "arguments": {}
}
```

### Fetch metrics at a specific point in time

```json
{
  "tool": "get_metrics",
  "arguments": {
    "at": "2024-09-18T03:15:00Z"
  }
}
```

## Implementation Notes

- Follow the Rust server conventions established in `mcp/structured-memory` and `mcp/scheduler` (Tokio runtime, tool registry, MCP transport).
- Use `reqwest` for HTTPS requests and parse the multi-line environment variable during startup.
- VictoriaMetrics is API-compatible with Prometheus, so standard `/api/v1/query` semantics apply; range queries are intentionally unsupported.
