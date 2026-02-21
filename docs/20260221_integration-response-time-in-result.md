# Integration Result Response-Time Fields

## Context
- Integration result files were useful for quality gates, but they did not expose runtime responsiveness.
- Performance diagnosis required reading verbose runtime logs (`PERF ...`) instead of structured test artifacts.

## Decision
- Added response-time fields to each integration run result:
  - `response_time_ms_by_turn`: per-turn latency in milliseconds.
  - `response_time_ms_mean`: arithmetic mean of turn latencies.
  - `response_time_ms_min`: minimum turn latency.
  - `response_time_ms_max`: maximum turn latency.

## Measurement Definition
- Turn response time is measured from the moment the tester sends a user message over WebSocket
  until the first accepted assistant reply event (`action,response`) is received for that turn.
- Values are recorded in milliseconds.

## Why
- Keeps latency diagnostics in the same artifact as pass/fail and judge metrics.
- Makes regression checks easier without parsing raw runtime logs.
