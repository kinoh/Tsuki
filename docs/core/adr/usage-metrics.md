---
date: 2026-03-06
---

# ADR: Usage Metrics — Dependency Inversion for Recording

## Context

`core-rust` had no stable usage tracking. Debug raw events were the only source, providing no
stable aggregate values for cost/volume visibility.

## Decision

- Dedicated `usage_stats` table in SQLite.
- `LlmUsageRecorder` trait abstracts persistence; `llm.rs` depends on the abstraction, not on DB.
- `DbLlmUsageRecorder` is the production implementation, injected via `ResponseApiConfig`.
- `GET /metrics` returns lifetime cumulative totals (no time-range query).

Response shape:
```json
{
  "total_messages": 789,
  "total_users": 12,
  "usage": {
    "input_tokens": 500000,
    "output_tokens": 120000,
    "total_tokens": 620000,
    "reasoning_tokens": 30000,
    "cached_input_tokens": 80000
  }
}
```

Token field names are kept as-is (`input_tokens`, `output_tokens`, etc.) for vocabulary
consistency across code, API, and operations.

## Rationale

Dependency inversion keeps the LLM adapter decoupled from storage. Lifetime cumulative is simpler
and sufficient for operational visibility. Explicit table contract is preferred over fallback
aggregation from event logs.

## Known Limitation

`total_users` reflects distinct stored usage user IDs, not authenticated connection principals,
because per-session identity is not currently propagated through the pipeline input contract.

## Compatibility Impact

breaking-by-default (no compatibility layer)
