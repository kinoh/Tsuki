# Core-Rust Replacement Migration Checklist

## Overview
This document defines the concrete migration tasks required to replace `core/` (TypeScript) with `core-rust/` as the production backend.

It reflects the currently agreed scope:
- Do not provide TTS as an API for now.
- Remove thread-based history APIs/concepts.
- Use `/events` as the history retrieval interface.
- Migrate legacy conversation history into event rows.
- Drop tool/reasoning intermediate artifacts during migration.
- Preserve original message timestamps when importing legacy history.

Compatibility Impact: breaking-by-default (no compatibility layer)

## Goals
- Replace production runtime from `core/` to `core-rust/`.
- Keep WebSocket chat behavior working for clients.
- Replace thread/message history access with event-log access.
- Ensure historical continuity by importing legacy messages into event storage.

## Non-Goals
- Re-implementing AdminJS feature parity in this migration step.
- Providing `/tts` API.
- Preserving `threads` semantics or compatibility wrappers.

## Contract Changes
- Remove/deprecate:
  - `GET /threads`
  - `GET /threads/:id`
  - `GET /messages`
  - `POST /tts`
- Provide/standardize:
  - `GET /events` (read model for history)
  - Existing WebSocket ingress for user input and egress for runtime events

## Migration Plan

### 1. API Surface Consolidation
- [ ] Define and document `/events` query contract (minimum required filters, pagination, ordering).
- [ ] Implement production-grade `/events` endpoint in `core-rust` (not debug-only path).
- [ ] Remove `threads/messages/tts` routes from production-facing API map.
- [ ] Update API references (`README`, specs, ops docs) to event-centric history.

### 2. WebSocket Contract Alignment
- [ ] Reconcile current `core-rust` WebSocket payload shape with `api-specs/asyncapi.yaml`.
- [ ] Ensure outbound message contract is explicit for clients consuming event stream.
- [ ] Add compatibility tests for auth handshake and message ingest (`message`, `sensory`).

### 3. Legacy History Import (Mastra -> Event Store)
- [ ] Add a migration tool to read historical messages from Mastra/libSQL sources.
- [ ] Map each legacy message to one `Event` row with source/modality/tags policy.
- [ ] Exclude tool/reasoning internals from imported dataset.
- [ ] Preserve original message timestamp as `event.ts`.
- [ ] Add idempotency strategy (re-run safe import with duplicate prevention).
- [ ] Add import report output (imported count, skipped count, error count).

### 4. Data Semantics and Read Model
- [ ] Define canonical event tagging for imported messages (for example: `imported_legacy`, `user_input`, `assistant_output`).
- [ ] Confirm ordering semantics for mixed live/imported events.
- [ ] Confirm `/events` default sort and stable pagination behavior.

### 5. Runtime and Deployment Switch
- [ ] Switch `compose.yaml` primary backend service to `core-rust`.
- [ ] Update healthcheck to validate `core-rust`-owned readiness.
- [ ] Update Taskfile runtime/deploy commands if service names or startup flows change.
- [ ] Validate required env/config mapping for `core-rust` in production.

### 6. Client and Consumer Updates
- [ ] Update GUI/API consumers to use `/events` instead of thread/message APIs.
- [ ] Remove thread-dependent assumptions from client state model.
- [ ] Verify timeline/history views from event stream only.

### 7. Verification and Cutover
- [ ] Add end-to-end checks: WebSocket message flow + `/events` retrieval.
- [ ] Run history-import validation on a representative backup dataset.
- [ ] Execute cutover rehearsal in staging-like environment.
- [ ] Define rollback criteria and rollback procedure.
- [ ] Perform production cutover and post-cutover smoke checks.

## Acceptance Checklist (Definition of Done)
- [ ] No production dependency remains on `threads` or `messages` APIs.
- [ ] `/events` is the sole history retrieval API in active clients.
- [ ] Legacy conversation history is imported with original timestamps.
- [ ] Imported dataset excludes tool/reasoning internals by design.
- [ ] WebSocket chat loop works with agreed auth and payload contracts.
- [ ] Compose/Taskfile operational path starts `core-rust` as primary backend.
- [ ] Runbook/docs are updated for on-call and routine operations.

## Open Decisions (Must Be Closed Before Cutover)
- [ ] Final `/events` query schema (filters, limits, around-window behavior).
- [ ] Event tag taxonomy for UI and analytics consumers.
- [ ] Exact rollback trigger thresholds (error rate, latency, message failure rate).
