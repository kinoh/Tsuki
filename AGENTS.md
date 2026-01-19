# AGENTS.md

Guidance for coding agents working on Tsuki.

## Concept & Experience
- Tsuki is a kawaii, conversational agent; aim for casual, non-productive chatter and playful presence.
- Router is a pre-conscious filter; repeated sensory inputs may pass to surface new facets later. Strict dedupe is intentionally avoided.
- Liveliness > determinism; allow variability and small surprises.

## Directory Layout (high level)
- `core/`: TypeScript backend on Mastra
  - `src/agent/`: router, responder, ActiveUser orchestration
  - `src/server/`: HTTP + WebSocket server, routes, middleware
  - `src/mastra/`: Mastra setup, MCP wiring
  - `src/storage/`: LibSQL and usage tracking
  - `scripts/`: prompt encrypt/decrypt, manual checks
- `gui/`: Tauri + Svelte client
  - `src/`: Svelte routes/components (e.g., `routes/+page.svelte`, `Config.svelte`, `Status.svelte`, `Note.svelte`)
  - `src-tauri/`: Tauri shell (Rust) with `src/main.rs`, `lib.rs`, `tauri.conf.json`, `capabilities/`, `icons/`
  - `static/`: packaged static assets
- `api-specs/`: AsyncAPI for WebSocket protocol
- `docs/`: design decisions and change logs (see docs/README.md for documentation strategy)
- `docker/`, `compose.yaml`, `Taskfile.yaml`: container and task runner configs

## System Shape
- Core (in `core/`): Mastra/TypeScript backend; exposes HTTP + WebSocket; channels: websocket, FCM, internal.
- Client (in `gui/`): Tauri + Svelte desktop/mobile client.
- MCP-first: abilities come from MCP servers, not built-ins.
- Per-user orchestration: `ActiveUser` holds conversation state, router, responder, and MCP client.

## Interfaces
- HTTP API: see `core/src/server/routes` for endpoints and middleware.
- WebSocket: see `api-specs/asyncapi.yaml` (AsyncAPI spec).
- Admin UI: AdminJS at `/admin`, authenticated via `WEB_AUTH_TOKEN`.

## MCP Topology
- Universal MCP
  - `rss-mcp-lite` (npm) for shared RSS; data: `${DATA_DIR}/rss_feeds.db`, `${DATA_DIR}/rss_feeds.opml`.
- User-specific MCP (Rust binaries)
  - `scheduler`: time-based notifications, data under `${DATA_DIR}/${userId}__scheduler/`.
- MCP clients support resource subscriptions; isolation is per user; roots are under `DATA_DIR`.

## Runtime & Commands
- Core dev
  - `cd core && pnpm start` (tsx dev)
  - `pnpm run start:prod`
  - `pnpm run lint`
- GUI dev
  - `cd gui && npm run dev`
- Docker/Taskfile
  - `task up`, `task deploy-core`
  - `docker compose up --build --detach`

## Config & Data
- Core env: `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `OPENAI_MODEL`, `AGENT_NAME`, `PROMPT_PRIVATE_KEY`, `DATA_DIR` (default `./data`), `TZ`.
- Optional env: `GCP_SERVICE_ACCOUNT_KEY`, `FCM_PROJECT_ID`, `PERMANENT_USERS`, `ADMIN_JS_TMP_DIR`, `SENSORY_POLL_SECONDS`, `ROUTER_HISTORY_LIMIT`.
- Storage roots: Mastra DB `${DATA_DIR}/mastra.db`; RSS DB under `${DATA_DIR}`; encrypted prompts in `core/src/prompts/`; AdminJS temp dir default `/tmp/.adminjs`.

## Testing
- Static checks: `pnpm run lint`.
- Manual scripts (run only when requested; may consume API or external calls):
  - `pnpm run test:agent`
  - `node scripts/test_memory.js`
  - `tsx scripts/test_reflection.ts`
  - `node scripts/mcp_subscribe.js`
  - `node scripts/ws_client.js`
