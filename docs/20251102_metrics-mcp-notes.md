# Metrics MCP README Worklog

## Overview
- Branch `feat/metrics-mcp` introduces documentation for a future Prometheus/VictoriaMetrics MCP server.
- Work limited to drafting `mcp/metrics/README.md`; no code scaffold yet.

## Instruction Trail & Rationale
- **Initial brief**: Create a concise README similar to other MCP servers. Goal: server returns fixed metrics via a tool, defaulting to latest values when timestamp omitted.
- **User feedback 1**: Avoid external files for query catalogue; prefer multi-line environment variable to simplify Docker deployment. Result: replaced `METRICS_CONFIG_PATH` with `METRICS_QUERIES` using `name=query` lines and removed metric `id` filtering.
- **User feedback 2**: Range queries deemed unnecessary; removed `DEFAULT_STEP`.
- **User feedback 3**: Responses should be summarised (`__name__` + value) and formatted with TOON. README updated accordingly and clarified output schema.
- **User feedback 4**: Example TOON block corrected by user; README aligned with provided format, emphasising local-time timestamps governed by `TZ`.
- **Additional config**: Document now requires `TZ` to express timestamps in the agent’s local timezone for LLM friendliness.

## Improvement Notes
- Early draft assumed file-based config and optional metric selection; user guidance shifted design to environment-driven, all-metrics output—simplifies ops but required README rewrite.
- Timestamp handling refined from epoch seconds to RFC3339 local time to better support downstream reasoning.
- README explicitly references TOON to ensure future implementation matches expected serialization.
