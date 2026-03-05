# Core-Rust Usage Metrics

## Overview
Implemented usage-token persistence and `/metrics` for `core-rust` to provide lifetime cumulative operational visibility.

## Problem Statement
`core` had token usage storage and a metrics endpoint, but `core-rust` had no equivalent storage contract or metrics API.
Without a dedicated usage table, cost/volume tracking relied on debug raw events and could not provide stable aggregate values.

## Solution
- Added a dedicated `usage_stats` table in `core-rust`.
- Extracted usage fields from OpenAI response payloads in `llm.rs`.
- Added `LlmUsageRecorder` interface and moved usage recording into `ResponseApiAdapter` (single point).
- Added `GET /metrics` endpoint returning lifetime cumulative metrics.

## API Contract
`GET /metrics` returns:

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

### Naming Decision
Kept existing token field names unchanged:
- `input_tokens`
- `output_tokens`
- `total_tokens`
- `reasoning_tokens`
- `cached_input_tokens`

Reason: preserve vocabulary consistency across code, API, and operations. Renaming these fields was explicitly rejected during review.

## Design Decisions
- Responsibility boundaries:
  - `llm.rs`: extract usage from response payload and invoke `LlmUsageRecorder`
  - `db.rs`: persist usage and aggregate metrics
  - `application/usage_service.rs`: DB-backed `LlmUsageRecorder` implementation
  - `server_app.rs`: expose `/metrics`
- Applied dependency inversion for usage persistence:
  - high-level `llm` module depends on `LlmUsageRecorder` abstraction, not on DB implementation.
  - low-level DB implementation (`DbLlmUsageRecorder`) is injected via `ResponseApiConfig`.
- No time-range query (`from/to`) was introduced; endpoint is lifetime cumulative by design.
- No fallback aggregation from event logs; fail-fast and explicit table contract is preferred.

## Compatibility Impact
Breaking-by-default policy remains intact. New endpoint and table are additive; no compatibility layer or fallback path was introduced.

## Trade-offs and Limitations
- Current runtime input contract does not carry per-session authenticated user identity into pipeline processing.
  Usage currently records a stable logical user id (`user`) for aggregation consistency.
- As a result, `total_users` reflects distinct stored usage user IDs, not authenticated connection principals.

## Future Considerations
- If per-user metrics are required, propagate authenticated principal through pipeline input contract and usage recording.
- Consider adding separate operational counters if internal module-only usage must be split from user-facing interactions.
