# Runtime Config (Core)

## Decision
- Define the HTTP config API in OpenAPI (`api-specs/openapi.yaml`) with `GET /config` and `PUT /config`.
- Generate TypeScript types from OpenAPI using `openapi-typescript` into `core/src/shared/openapi.ts`.
- Store global runtime config in `${DATA_DIR}/config.json` with defaults enabled.
- Apply changes immediately: sensory polling starts/stops on update; notifications are skipped when disabled.

## Notes
- Payload is minimal: `enableNotification` and `enableSensory` only.
- `PUT /config` requires both fields; no PATCH support.
- Generation runs via `pnpm -C core run generate:openapi`; Docker build no longer generates types.

## User Feedback Incorporated
- Use OpenAPI as the source of truth and generate types from it.
- Keep settings global, not per-user.
- Persist to `config.json` under `DATA_DIR`.
- Apply changes without restart.
- Keep `openapi.ts` in git and remove Docker-based generation.
