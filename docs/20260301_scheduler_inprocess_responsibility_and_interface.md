# Core-Rust In-Process Scheduler Responsibility and Interface

## Overview
This document defines the scheduler design for `core-rust` with explicit responsibility boundaries and a minimal runtime/tool interface.

Compatibility Impact: breaking-by-default (introduces a new in-process scheduler contract with no compatibility layer for MCP-subscription-based scheduling).

Status: planned change (design-approved, implementation not yet merged).

## Problem Statement
Current scheduling behavior is split from `core-rust` runtime semantics:
- self-improvement trigger execution already depends on runtime events (`self_improvement.run`)
- scheduler transport complexity (MCP subscription/read path) adds operational overhead for a tightly-coupled internal concern
- cron-based interfaces are fragile for LLM-driven schedule authoring
- duplicated scheduling/dispatch history stores increase inconsistency risk

We need a single in-process model where:
- auto-trigger policy is configuration-driven
- runtime emission stays event-native
- duplicate firing is prevented without introducing a second history source

## Solution
Adopt an in-process scheduler in `core-rust` with:
- configuration bootstrap (`config.toml`) for default schedules
- non-cron structured recurrence schema for both external tool input and internal storage
- one runtime engine loop that computes due schedules and emits scheduled events through a single function
- duplicate prevention by querying existing event history before event emission

Google Calendar integration is explicitly out of scope in this phase.

## Design Decisions
### 1. Responsibility boundaries
- `ScheduleStore` owns schedule persistence and due-selection state (`next_fire_at`).
- `SchedulerEngine` is the primary owner of execution flow:
  - load due schedules
  - build emission input
  - call emission function
  - update next schedule state
- Emission is handled by a function (`emit_scheduled_event`) instead of a dedicated dispatcher object.
  - Why: no extra ownership layer is required; function-level boundary is sufficient.

### 2. Tool interface is minimal and upsert-centered
- Expose only:
  - `schedule_upsert`
  - `schedule_list`
  - `schedule_remove`
- Do not expose `enable/disable` as separate tools.
  - Why: `enabled` is part of the `schedule_upsert` payload.

### 3. No external `owner` field
- Tool/config inputs must not carry free-form `owner`.
- Runtime derives owner scope from authenticated context (or fixed bootstrap scope).
  - Why: avoids ambiguous or spoofable ownership input while preserving internal multi-user partitioning.

### 4. No cron in interface or storage
- Recurrence must be structured (`once|daily|weekly|monthly|interval`) instead of raw cron text.
  - Why: improves LLM reliability, validation quality, and human reviewability.

### 5. Duplicate prevention uses event history only
- Do not create a separate fired-history store/table.
- Before emitting, query event history for existing `scheduler.fired` with:
  - `payload.schedule_id`
  - `payload.scheduled_at`
- If found, skip emission.
  - Why: keep event stream as single source of truth and avoid dual-write divergence.

### 6. No synthetic `dispatch_key`
- Do not persist an extra dispatch identifier when `schedule_id + scheduled_at` is already sufficient.
  - Why: avoid redundant identifiers and inconsistency risk.

### 7. Auto-trigger policy is config-driven
- Initial schedules must be declared in `config.toml` bootstrap entries.
- Runtime must fail fast when bootstrap entries are invalid.
  - Why: policy belongs to configuration, not hardcoded runtime behavior.

## Implementation Details
### Planned runtime components
- `ScheduleStore`
  - `upsert(schedule)`
  - `list(scope)`
  - `remove(scope, id)`
  - `acquire_due(now, limit)`
  - `update_next_fire(scope, id, next_fire_at)`
- `SchedulerEngine`
  - interval tick loop
  - due selection
  - emission call
  - next-fire update
- `emit_scheduled_event(input)`
  - query event history for duplicate (`scheduler.fired`, `schedule_id`, `scheduled_at`)
  - on miss: emit scheduled event and record `scheduler.fired`
  - on hit: skip as duplicate

### Planned tool payload shape (non-cron)
```json
{
  "id": "daily_self_improvement",
  "recurrence": {
    "kind": "daily",
    "at": "04:00:00"
  },
  "timezone": "Asia/Tokyo",
  "action": {
    "kind": "emit_event",
    "event": "self_improvement.run",
    "payload": {
      "target": "all",
      "reason": "scheduled_daily"
    }
  },
  "enabled": true
}
```

### Planned bootstrap config shape
```toml
[scheduler]
enabled = true
tick_interval_ms = 1000

[[scheduler.bootstrap]]
id = "daily_self_improvement"
enabled = true
timezone = "Asia/Tokyo"

[scheduler.bootstrap.recurrence]
kind = "daily"
at = "04:00:00"

[scheduler.bootstrap.action]
kind = "emit_event"
event = "self_improvement.run"

[scheduler.bootstrap.action.payload]
target = "all"
reason = "scheduled_daily"
```

### Planned emitted event contract
- event/tag: `scheduler.fired`
- payload minimum:
  - `schedule_id`
  - `scheduled_at`
  - `fired_at`
  - `action.event`
  - `action.payload`
- additional routing tag: action event name (for example `self_improvement.run`)

## Future Considerations
- If runtime becomes multi-instance, duplicate checks must be transaction-safe at storage/query boundary.
- If recurrence needs richer calendars later, extend structured recurrence types rather than introducing cron text as default.
- External calendar adapters (Google Calendar, etc.) should remain separate adapters that call scheduler upsert/remove interfaces.

## Supersedes
- Scheduler-scope part of `docs/20260207_self-improvement-phase-event-native-design.md` that marked in-process daily scheduler as out of scope.
