# Core-Rust Production Runbook

## Overview
This runbook defines routine operations and post-cutover checks for the production `core-rust` backend.

Cutover status:
- Production backend switched from `core` (TypeScript) to `core-rust` on **2026-02-28**.
- Active client scope is **GUI only** at the time of writing.

Compatibility Impact: breaking-by-default (no compatibility layer)

## Scope
- Validate production health and chat-path functionality.
- Verify migration correctness for event history and concept graph data.
- Validate notification delivery behavior.
- Define first-response checks for common operational failures.

## Prerequisites
- Access to production logs and deployment environment.
- Valid `Authorization` header format: `<user>:<token>`.
- A test device with the production app installed for notification checks.

## Routine Health Checks
1. Verify runtime metadata is reachable.
   - `GET /metadata` returns `200`.
2. Verify history API is reachable.
   - `GET /events?limit=20&order=desc` returns `200` and non-error payload.
3. Verify WebSocket chat path.
   - Connect to `ws://` or `wss://` root path (`/`), send auth frame, send a user message, confirm runtime events are received.
4. Verify runtime config persistence.
   - `PUT /config` with a known value.
   - Restart `core`.
   - `GET /config` confirms the same value after restart.

## Notification Checks
1. Register a token via `PUT /notification/token`.
2. Confirm registration via `GET /notification/tokens`.
3. Trigger test delivery via `POST /notification/_test`.
4. Confirm push reception on a production-installed device.

Current production verification:
- `POST /notification/_test` was confirmed to deliver to a production-installed device.

## Migration Verification

### Event Store Migration (`/admin/events`)
1. Validate imported dataset integrity.
   - Compare expected import counters (`processed`, `imported`, `dropped`, `failed`) with observed rows.
2. Validate timestamp preservation.
   - Source legacy `createdAt` min/max must match event `ts` min/max.
3. Validate exclusion policy.
   - Tool/reasoning intermediate artifacts are not present in imported event payloads.

### Concept Graph Migration (`/admin`)
1. Verify counts for concept graph entities (nodes/relations/episodes) against expected snapshot.
2. Validate representative records for each entity type.
3. Confirm query/read paths return consistent results without user-scope mixing.

## Deployment-Time Manual Requirement
`prompts.md` must be manually placed in production data storage before startup.

- Required path: `/data/prompts.md`.
- Behavior if missing: startup fails fast by design (no fallback persona source).
- Deployment checklist must include explicit file presence verification.

## Incident First Response

### WebSocket auth failure
Signals:
- `WS_AUTH_FAIL reason=invalid_token`
- A preceding `HTTP_ACCESS path=/ status=101` can still appear due to transport upgrade before auth rejection.

Actions:
1. Validate token value and user/token pair format.
2. Re-check the client-side auth frame content.
3. Check recent deployment/config changes affecting auth secrets.

### Service health and logs
- `task ps`
- `task log-core`

## Dependency Retirement Verification
Production access logs must show no active dependency on legacy history APIs:
- `/threads`
- `/messages`

Current production verification:
- No `/threads` or `/messages` access observed in production logs after cutover.

## Required Runtime Mapping (Operations Reference)

### Runtime container environment (`compose.yaml`)
- Required: `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `ADMIN_AUTH_PASSWORD`
- Optional (notifications): `FCM_PROJECT_ID`, `GCP_SERVICE_ACCOUNT_KEY`

### CI/CD secret mapping (`.github/workflows/deploy.yml`)
- `WEB_AUTH_TOKEN`
- `OPENAI_API_KEY`
- `ADMIN_AUTH_PASSWORD`
- `GCP_SERVICE_ACCOUNT_KEY`
- `FCM_PROJECT_ID`
