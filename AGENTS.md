# AGENTS.md

Guidance for coding agents working on Tsuki.

## Concept & Experience
- Tsuki is a kawaii, conversational agent; aim for casual, non-productive chatter and playful presence.
- Router is a pre-conscious filter; repeated sensory inputs may pass to surface new facets later. Strict dedupe is intentionally avoided.
- Liveliness > determinism; allow variability and small surprises.

## Directory Layout (high level)
- `core/`: Rust backend runtime — see `core/AGENTS.md` for implementation rules
  - `src/application/`: application services and orchestration boundaries
  - `src/server_app.rs`: HTTP + WebSocket server, admin/auth surfaces, route wiring
  - `src/bin/`: operational and maintenance CLIs
  - `tests/integration/`: integration harness, scenarios, and operator notes
  - `static/`: runtime-served HTML/CSS assets
- `mcp/`: MCP server/provider implementations and their tool contracts
  - Treat tool descriptions, schemas, and server-specific behavior as owned by each provider here
- `gui/`: Tauri + Svelte client
  - `src/`: Svelte routes/components (e.g. `routes/+page.svelte`, `Config.svelte`, `Status.svelte`, `Note.svelte`)
  - `src-tauri/`: Tauri shell (Rust) with `src/main.rs`, `lib.rs`, `tauri.conf.json`, `capabilities/`, `icons/`
  - `static/`: packaged static assets
- `api-specs/`: OpenAPI and AsyncAPI contracts
- `docs/`: specs and ADRs by area — `core/`, `gui/`, `infra/`, `integrations/` (see `docs/README.md`)
- `docker/`, `compose.yaml`, `Taskfile.yaml`: container and task runner configs

## System Shape
- Backend (in `core/`): Rust runtime exposing HTTP + WebSocket, notifications, admin/auth, and MCP-backed execution.
- Client (in `gui/`): Tauri + Svelte desktop/mobile client.
- MCP-first: abilities come from MCP servers, not built-ins.
- Per-user runtime state is stored in the event/database layer rather than in a TypeScript thread manager.

## Interfaces
- HTTP API: see `api-specs/openapi.yaml`.
- WebSocket: see `api-specs/asyncapi.yaml`.
- Admin UI: `/admin` with login at `/admin/login`, authenticated by `ADMIN_AUTH_PASSWORD` session flow.

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
- When an MCP tool contract feels wrong (description, schema, examples, behavior), inspect the provider implementation under `mcp/` before patching a consumer such as `core`.

## Runtime & Commands
- Backend dev
  - `cd core && cargo run`
  - `cd core && cargo test`
- GUI dev
  - `cd gui && npm run dev`
- Docker/Taskfile
  - `task up`
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
  - Before finishing, verify there is no missing propagation using code search (e.g. `rg`) across runtime config and workflow files.

## Infrastructure & Operations
- Production `DOCKER_HOST` is defined in `.env`; `task` picks it up automatically via `dotenv`; never read or hardcode the value directly.
- Stop the `core` container before restoring a Memgraph snapshot to prevent concurrent writes during restore.
- Memgraph vector index creation retroactively scans existing nodes, but aborts on the first dimension mismatch and leaves the index partially populated with no client-visible error; always verify `SHOW VECTOR INDEX INFO` size equals the expected node count after creation.
- Bulk-loading vector properties via `SET` before a vector index exists stores the data but does not index it; drop and recreate the index after bulk load to force full indexing.

## Config & Data
- Required runtime env: `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `ADMIN_AUTH_PASSWORD`.
- Optional runtime env: `FCM_PROJECT_ID`, `GCP_SERVICE_ACCOUNT_KEY`, `GEMINI_API_KEY`, `MEMGRAPH_PASSWORD`, `TURSO_AUTH_TOKEN`.
- Dev/test-only env may include `OPENAI_MODEL` and `PROMPT_PRIVATE_KEY` for auxiliary tools.
- Storage roots: runtime DB `${DATA_DIR}/core.db`, shared RSS data under `${DATA_DIR}`, prompt file `${DATA_DIR}/prompts.md`.

## Testing
- Rust tests: `cd core && cargo test`.
- Integration harness: `task -t core/Taskfile.yaml integration/run -- --scenario tests/integration/scenarios/chitchat.yaml --run-count 1`
