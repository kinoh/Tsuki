# Core-Rust Replacement Migration Checklist

## Overview
This document defines the concrete migration tasks required to replace `core/` (TypeScript) with `core-rust/` as the production backend.

It reflects the currently agreed scope:
- Remove thread-based history APIs/concepts.
- Use `/events` as the history retrieval interface.
- Implement runtime configuration API (`/config`) required by clients.
- Implement notification capability required by clients.
- Keep sensory acquisition/polling out of scope for this migration phase.
- Migrate legacy conversation history into event rows.
- Drop tool/reasoning intermediate artifacts during migration.
- Preserve original message timestamps when importing legacy history.
- Do not introduce cursor pagination in this phase.
- Do not store `legacy_message_id` / `legacy_thread_id` in migrated event payloads.
- Event DB is rebuilt from zero for this branch, so import idempotency and import rollback mechanisms are out of scope.

Compatibility Impact: breaking-by-default (no compatibility layer)

## Goals
- Replace production runtime from `core/` to `core-rust/`.
- Keep WebSocket chat behavior working for clients.
- Replace thread/message history access with event-log access.
- Ensure historical continuity by importing legacy messages into event storage.

## Contract Changes
- Remove/deprecate:
  - `GET /threads`
  - `GET /threads/:id`
  - `GET /messages`
- Provide/standardize:
  - `GET /events` (read model for history)
  - `POST /tts` (text-to-speech synthesis)
  - Existing WebSocket ingress for user input and egress for runtime events

## Migration Plan

### 1. API Surface Consolidation
- [x] Define and document `/events` query contract (minimal: `limit`, `before_ts`, `order`).
- [x] Implement production-grade `/events` endpoint in `core-rust` (not debug-only path).
- [x] Implement `/config` API (`GET`/`PUT`) with auth and persistent runtime config storage (`enableSensory` remains accepted for compatibility; sensory acquisition itself is out of scope).
- [x] Implement notification APIs required by current clients (`/notification/token`, `/notification/tokens`, `/notification/_test`) including actual notification delivery.
- [x] Keep `core` legacy routes as-is for now (route removal is out of scope in this migration phase).
- [x] Update active protocol reference needed for this phase (`api-specs/asyncapi.yaml`) to event-stream contract.
- [x] Implement `/metadata` with core-compatible fields plus `router_model`, `active_modules`, and API spec versions from `api-specs/asyncapi.yaml` + `api-specs/openapi.yaml`.

### 2. WebSocket Contract Alignment
- [x] Reconcile current `core-rust` WebSocket payload shape with `api-specs/asyncapi.yaml`.
- [x] Ensure outbound message contract is explicit for clients consuming event stream.
- [x] Add tests for auth handshake and message ingest (`message`, `sensory`).

### 3. Legacy History Import (Mastra -> Event Store)
- [x] Add a migration tool to read historical messages from Mastra/libSQL sources.
- [x] Map each legacy message to one `Event` row with source/modality/tags policy.
- [x] Exclude tool/reasoning internals from imported dataset.
- [x] Preserve original message timestamp as `event.ts`.
- [x] Add import report output (processed/imported/dropped/failed counts).

### 4. Data Semantics and Read Model
- [x] Define canonical event tagging for imported messages:
  - all imported rows: `imported_legacy`
  - imported user rows: `user_input`
  - imported assistant rows: `response`
  - imported system rows: `system_output`
- [x] Confirm ordering semantics for mixed live/imported events:
  - query order is controlled by `order=asc|desc` (`desc` default)
  - DB-level sort key is `(ts, event_id)` for deterministic ordering on identical timestamps
  - pagination for this phase is `before_ts + limit` (no cursor)
- [x] Confirm `/events` default sort behavior (`desc` by default; `asc|desc` selectable).
- [x] Confirm `/events` pagination policy for this phase (no cursor; `before_ts + limit` only).

### 5. Runtime and Deployment Switch
- [x] Switch `compose.yaml` primary backend service to `core-rust`.
- [x] Update healthcheck to validate `core-rust`-owned readiness.
- [x] Update Taskfile runtime/deploy commands if service names or startup flows change.
- [x] Validate required env/config mapping for `core-rust` in production.
- [x] Validate runtime config persistence and notification behavior after restart.

Verification notes (2026-02-28):
- Taskfile change was not required because runtime service name remains `core`.
- Validated compose runtime env mapping for `core-rust`:
  - required at startup: `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`
  - optional for notification delivery: `FCM_PROJECT_ID`, `GCP_SERVICE_ACCOUNT_KEY`
- Runtime persistence check passed after `core` restart:
  - `/config` value persisted (`enableNotification=true`, `enableSensory=false`)
  - `/notification/tokens` list persisted (`token-persist-check-1`)

### 6. Client and Consumer Updates
- [x] Update GUI/API consumers to use `/events` instead of thread/message APIs.
- [x] Remove thread-dependent assumptions from client state model.
- [x] Verify timeline/history views from event stream only.
- [x] Verify Config UI round-trip against `/config` on `core-rust`.
- [x] Verify notification registration flow against `core-rust` notification API.

### 7. Verification and Cutover
- [x] Add end-to-end checks: WebSocket message flow + `/events` retrieval.
- [x] Run history-import validation on a representative backup dataset.
- [x] Execute cutover rehearsal in local production-like environment (no staging environment exists).
- [x] Perform production cutover and post-cutover smoke checks.

History-import validation notes (2026-02-28):
- Source backup: `backup/tsuki-backup-20260208231014.tar.gz` (`./mastra.db`).
- Import result:
  - `processed=1348`
  - `imported=1166`
  - `dropped_non_text=2`
  - `dropped_by_substring=180`
  - `failed=0`
- Tag/source mapping validated:
  - `["imported_legacy","response"]`: `584` (source=`assistant`)
  - `["imported_legacy","user_input"]`: `582` (source=`user`)
- Timestamp preservation validated:
  - source `min/max(createdAt)` = `2025-07-24T14:06:53.077Z` / `2026-02-05T06:54:00.493Z`
  - target `min/max(ts)` = `2025-07-24T14:06:53.077Z` / `2026-02-05T06:54:00.493Z`
- Substring exclusion validated on target payload text (`count=0`):
  - `"modality":"None"`
  - `Received scheduler notification`

Local production-like rehearsal notes (2026-02-28):
- Rehearsal baseline: compose runtime switched to `core-rust` as primary backend.
- GUI message send/receive path validated in this branch.
- Runtime persistence validated across restart:
  - `/config` persistence
  - `/notification/tokens` persistence
- Startup fail-fast validated for missing `/data/prompts.md` (no fallback persona source).

Production cutover notes (2026-02-28):
- Production backend switched to `core-rust`.
- WebSocket send/receive path validated.
- Runtime config persistence validated after restart.
- WebSocket auth failure behavior validated:
  - `WS_AUTH_FAIL reason=invalid_token` observed.
  - Corresponding transport upgrade log observed (`HTTP_ACCESS path=/ status=101` before auth rejection).
- Notification delivery validated on production-installed client via `POST /notification/_test`.

## Acceptance Checklist (Definition of Done)
- [x] No production dependency remains on `threads` or `messages` APIs.
- [x] `/events` is the sole history retrieval API in active clients.
- [x] Legacy conversation history is imported with original timestamps.
- [x] Imported dataset excludes tool/reasoning internals by design.
- [x] WebSocket chat loop works with agreed auth and payload contracts.
- [x] Compose/Taskfile operational path starts `core-rust` as primary backend.
- [x] Runbook/docs are updated for on-call and routine operations.

Acceptance notes (2026-03-01):
- Active production client scope is GUI only.
- Production access logs show no `/threads` or `/messages` usage after cutover.
- `/admin/events` review indicates migrated event dataset integrity and exclusion policy hold.
- `/admin` review indicates concept graph migration is in expected state.
- On-call and routine operations runbook added:
  - `docs/20260301_core-rust-production-runbook.md`
