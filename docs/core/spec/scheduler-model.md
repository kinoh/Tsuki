# Scheduler Model

## Design Principle

The scheduler is in-process and event-native. Scheduled events are emitted into the event stream;
the event stream is the single source of truth for fired history. No separate dispatch table exists.

## Recurrence

Recurrence is structured, not cron text:
`once | daily | weekly | monthly | interval`

Structured recurrence improves LLM reliability, validation quality, and human reviewability. Cron
text is explicitly excluded from both the tool interface and internal storage.

## Duplicate Prevention

Before emitting a scheduled event, the scheduler queries the event stream for an existing
`scheduler.fired` event with matching `schedule_id` and `scheduled_at`. If found, emission is
skipped. No separate fired-history table exists.

## Tool Interface

Minimal and upsert-centered:
- `schedule_upsert` — create or update a schedule (includes `enabled` field)
- `schedule_list`
- `schedule_remove`

`enable` / `disable` are not separate tools — `enabled` is part of the upsert payload.

Actions: `emit_event` (runtime event emission) or `emit_message` (user-facing message).

## Owner Field

Tool inputs carry no free-form `owner`. Runtime derives scope from authenticated context.
This prevents spoofable or ambiguous ownership input.

## Auto-Trigger Policy

Self-improvement auto-trigger is declared in `config.toml` under `[scheduler.self_improvement]`.
Runtime fails fast when this section is invalid. Policy belongs to configuration, not hardcoded
runtime behavior.

## Extension Guidance

- New schedule action kinds belong in the `action.kind` enum, not as new tool surfaces.
- If multi-instance deployment is needed, duplicate prevention must become transaction-safe at
  the storage boundary.
- External calendar adapters (Google Calendar, etc.) should call `schedule_upsert/remove` as
  clients — they must not bypass the scheduler's duplicate prevention logic.
