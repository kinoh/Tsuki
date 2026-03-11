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
- `core-rust/`: Rust backend to replace `core/` (WIP)
- `mcp/`: MCP server/provider implementations and their tool contracts
  - Treat tool descriptions, schemas, and server-specific behavior as owned by each provider here
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

## Architecture Principles
- Always prioritize responsibility separation.
- Never introduce an event unless both of the following are explicit:
  - the domain the event is provided to
  - the module that owns responsibility for producing and maintaining it
- API contracts and event contracts are different by nature:
  - APIs are one-to-one contracts with clients
  - events are inter-module contracts for domain communication
  - do not force API naming or shape to mirror event naming mechanically
- Keep external input contracts minimal:
  - do not add optional/manual hint fields unless they are required by the operational model
- Any change to responsibility boundaries, event contracts, or API contracts must be reflected in docs on the same day.
- Backward compatibility is not required by default.
- Never adopt implicit fallbacks. Enforce fail-fast behavior for missing/invalid required behavior.
- Never swallow errors; always emit explicit logs with the concrete cause.
- Keep state simple; do not add state fields unless they are strictly necessary for domain behavior.
- When working in a git worktree, keep implementation, execution, verification, and cleanup scoped to that same worktree unless the user explicitly asks otherwise.
- Admin UIs must stay faithful to internal responsibility boundaries and must not mix cross-component derived data into a view for a single component.
- Migration/import tools must reuse the same event contract and normalization rules as runtime code; no ad-hoc mappings.
- Before writing or updating documents under `docs/`, always read `docs/README.md` and follow its documentation strategy.

## Communication baseline for changes
- Before proposing concrete tuning values, define numeric goals and show observed values that justify the change.
- Explain adjustments by the gap between target and observation, not by intuition.
- For disagreements or ambiguity, align first at design-philosophy level before discussing local implementation techniques.

## MCP Topology
- Universal MCP
  - `rss-mcp-lite` (npm) for shared RSS; data: `${DATA_DIR}/rss_feeds.db`, `${DATA_DIR}/rss_feeds.opml`.
- User-specific MCP (Rust binaries)
  - `scheduler`: time-based notifications, data under `${DATA_DIR}/${userId}__scheduler/`.
- MCP clients support resource subscriptions; isolation is per user; roots are under `DATA_DIR`.
- When an MCP tool contract feels wrong (description, schema, examples, behavior), inspect the provider implementation under `mcp/` before patching a consumer such as `core-rust`.

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
- Production deployment policy
  - Production rollout must go through CI/CD only; do not treat local `task`/`docker compose` execution as a production deployment path.
  - Always follow repository workflows under `.github/` for build, release, deployment, and post-deploy verification.
- Definition of Done (delivery path)
  - A change is not done until its delivery path is also updated and verified.
  - When introducing or changing required runtime env vars, update all relevant paths in the same change:
    - runtime wiring (`compose.yaml`, container runtime env)
    - CI/CD workflow env and secret mapping under `.github/workflows/`
    - operator-facing docs for required secrets/env
  - Before finishing, verify there is no missing propagation using code search (e.g., `rg`) across runtime config and workflow files.

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
