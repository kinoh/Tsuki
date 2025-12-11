# AGENTS.md

Guidance for coding agents working on Tsuki.

## Concept & Experience
- Tsuki is a kawaii, conversational agent; the goal is casual, non-productive chatter and a playful presence.
- The router acts as a pre-conscious filter: repeated sensory inputs may still pass so the core model can notice new facets later—strict dedupe is intentionally avoided.
- Liveliness is preferred over determinism; variability and small surprises are acceptable and desired.

## System Shape
- Core: TypeScript backend on Mastra in `core/`, exposes HTTP + WebSocket, multi-channel delivery (websocket, FCM, internal).
- Client: Tauri + Svelte app in `gui/`.
- MCP-first: abilities come from MCP servers rather than built-in tools.
- Per-user orchestration: `ActiveUser` holds conversation state, router, and responder.

## Interfaces
- HTTP API: see `core/src/server/routes` for the authoritative list of endpoints and middleware.
- WebSocket: protocol is specified in `api-specs/asyncapi.yaml` (AsyncAPI).
- Admin: AdminJS at `/admin`, authenticated via `WEB_AUTH_TOKEN`.

## MCP Topology
- Universal MCP: `rss-mcp-lite` (npm) for shared RSS feed management; data under `${DATA_DIR}/rss_feeds.db` and `${DATA_DIR}/rss_feeds.opml`.
- User-specific MCP (Rust binaries): `scheduler` (time-based notifications) and `structured-memory` (markdown notes), each stored under `${DATA_DIR}/${userId}__scheduler/` and `${DATA_DIR}/${userId}__structured_memory/`.
- MCP clients support resource subscriptions; isolation is per user and storage roots in `DATA_DIR`.

## Runtime & Commands
- Dev server: `cd core && pnpm start` (tsx runtime).
- Prod-like: `pnpm run start:prod`.
- Lint/typecheck: `pnpm run lint`.
- GUI dev: `cd gui && npm run dev`.
- Docker/Taskfile helpers for deploy/build: e.g., `task up`, `task deploy-core`, `docker compose up --build --detach`.

## Config & Data
- Core env: `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `OPENAI_MODEL`, `AGENT_NAME`, `PROMPT_PRIVATE_KEY`, `DATA_DIR` (default `./data`), `TZ`.
- Optional: `GCP_SERVICE_ACCOUNT_KEY`, `FCM_PROJECT_ID`, `PERMANENT_USERS`, `ADMIN_JS_TMP_DIR`.
- Key storage: Mastra DB `${DATA_DIR}/mastra.db`; RSS DB under `${DATA_DIR}`; encrypted prompt files in `core/src/prompts/`; AdminJS temp dir defaults to `/tmp/.adminjs`.

## Testing
- Manual scripts only; avoid API-consuming scripts without explicit request: `pnpm run test:agent`, `node scripts/test_memory.js`, `tsx scripts/test_reflection.ts`, `node scripts/mcp_subscribe.js`, `node scripts/ws_client.js`.
- Static checks: `pnpm run lint`.
