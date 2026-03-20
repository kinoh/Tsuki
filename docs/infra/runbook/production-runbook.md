# Production Runbook

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
   - Connect to root path (`/`), send auth frame, send a user message, confirm runtime events are received.
4. Verify runtime config persistence.
   - `PUT /config` with a known value, restart `core`, `GET /config` confirms the same value.

## Notification Checks

1. Register a token via `PUT /notification/token`.
2. Confirm registration via `GET /notification/tokens`.
3. Trigger test delivery via `POST /notification/_test`.
4. Confirm push reception on a production-installed device.

## Deployment Checklist

- `prompts.md` must be manually placed at `/data/prompts.md` before startup.
  Startup fails fast by design if missing — no fallback persona source exists.

## Incident First Response

### WebSocket auth failure

Signals:
- `WS_AUTH_FAIL reason=invalid_token`
- A preceding `HTTP_ACCESS path=/ status=101` may still appear (transport upgrade before auth rejection).

Actions:
1. Validate token value and user/token pair format.
2. Re-check client-side auth frame content.
3. Check recent deployment/config changes affecting auth secrets.

### Service health and logs

- `task ps`
- `task log-core`

## Runtime Environment Reference

### Container environment (`compose.yaml`)

| Variable | Required |
|---|---|
| `WEB_AUTH_TOKEN` | yes |
| `OPENAI_API_KEY` | yes |
| `ADMIN_AUTH_PASSWORD` | yes |
| `FCM_PROJECT_ID` | optional (notifications) |
| `GCP_SERVICE_ACCOUNT_KEY` | optional (notifications) |
| `GEMINI_API_KEY` | optional |
| `MEMGRAPH_PASSWORD` | optional |
| `TURSO_AUTH_TOKEN` | optional |

### CI/CD secret mapping (`.github/workflows/deploy.yml`)

- `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `ADMIN_AUTH_PASSWORD`
- `GCP_SERVICE_ACCOUNT_KEY`, `FCM_PROJECT_ID`
