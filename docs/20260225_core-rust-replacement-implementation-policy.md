# Core-Rust Replacement Implementation Policy

## Overview
This document defines the implementation policy to replace `core/` with `core-rust` under a minimal, migration-first scope.

The policy is intentionally optimized for fast cutover and reduced complexity.

Compatibility Impact: breaking-by-default (no compatibility layer)

## Fixed Scope
- Do not provide `/tts` API for now.
- Remove thread-based history APIs and semantics.
- Use `/events` as the only history retrieval API.
- Convert legacy conversation history into event rows.
- Drop tool/reasoning artifacts from migrated history.
- Preserve original message timestamps during import.

## API Policy

### `GET /events`
`/events` is the canonical read model for conversation history.

Query parameters:
- `limit` (optional, default `50`, max `500`)
- `before_ts` (optional, ISO8601)
- `order` (optional, `asc|desc`, default `desc`)

Out of scope for this phase:
- cursor-based pagination
- thread-scoped retrieval
- compatibility wrapper endpoints

Response shape:
- `items: Event[]`

Validation:
- Invalid query values must return `400`.
- No implicit fallback for malformed input.

## Event Normalization Policy (Legacy Import)
- Import unit is exactly one legacy message to one event row.
- `event.ts` must use the original message timestamp.
- `modality` is `text` for imported rows.
- `payload` is minimal and text-centric.
- Do not include `legacy_message_id` or `legacy_thread_id`.
- Drop legacy tool/reasoning message artifacts.

Source mapping:
- legacy `role=user` -> `source=user`
- legacy `role=assistant` -> `source=assistant`
- legacy `role=system` -> `source=system`
- legacy `role=tool` -> excluded from import

Tagging baseline:
- all imported rows: `imported_legacy`
- user rows: `user_input`
- assistant rows: `response`
- system rows: `system_output`

## Import Execution Policy
- Target event database is created from zero for cutover.
- Import is executed as a one-time full rebuild.
- Since this branch is not deployed yet, compatibility/idempotency/rollback handling is out of scope.
- Migration success is judged by completion + sampling verification.

Minimum verification:
- total imported count matches expected migrated scope
- sampled records preserve original timestamps
- sampled records preserve message text integrity

## WebSocket Policy
- Keep existing auth handshake (`USER:WEB_AUTH_TOKEN` first message).
- Client input contract: `type=message|sensory`.
- Server output contract: event stream (`type=event`) only.
- Invalid payload handling remains fail-fast at validation boundary.

## Removed Interfaces in This Replacement
- `GET /threads`
- `GET /threads/:id`
- `GET /messages`
- `POST /tts`

Clients and operational tooling must migrate to event-centric reads.

## Implementation Order
1. Finalize and implement `/events` production endpoint in `core-rust`.
2. Implement legacy message -> event import tool with fixed normalization rules.
3. Remove thread/message/tts routes from active production contract.
4. Update GUI and operational references to `/events`.
5. Run cutover verification and switch runtime entry to `core-rust`.
