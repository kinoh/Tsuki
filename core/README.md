# Tsuki Rust Core

Rust runtime for Tsuki. It exposes HTTP and WebSocket interfaces, persists runtime state in libSQL, and emits internal processing as event-stream data.

## Run
```bash
cd core-rust
cargo run
```

Config file:
- `config.toml` (required, no defaults)
- `llm.temperature_enabled` controls whether temperature is sent
- `prompts.path` must be explicitly configured
- `concept_graph.memgraph_uri` / `concept_graph.arousal_tau_ms` configure graph runtime
- `tts.*` configures VoiceVox/ja-accent endpoints and timeout/speaker

Environment variables (runtime):
- `WEB_AUTH_TOKEN` (required)
- `ADMIN_AUTH_PASSWORD` (required)
- `OPENAI_API_KEY` (required)
- `MEMGRAPH_PASSWORD` (required when Memgraph auth is enabled)
- `TURSO_AUTH_TOKEN` (required when `db.remote_url` is set)

## WebSocket CLI example
```bash
cd core-rust
WEB_AUTH_TOKEN=test-token WS_URL=ws://localhost:2953/ cargo run --example ws_client
```

First message is auth: `USER_NAME:WEB_AUTH_TOKEN`.
After auth, send JSON like:
```json
{"type":"message","text":"hello"}
```

You will receive event messages:
```json
{"type":"event","event":{...}}
```

## LLM-driven integration test assets
- Integration-test assets are isolated under `tests/integration/`.
- Use task orchestration from `core-rust/Taskfile.yaml`.
- Integration tests use isolated Memgraph in `compose.test.yaml` (`memgraph-test`, `bolt://localhost:7697`).
- Memgraph restore policy for integration setup uses latest snapshot (`integration/memgraph/restore/latest`).
- Harness entrypoint: `cargo run --example integration_harness -- --help`.
- Full run example:
  - `task -t core-rust/Taskfile.yaml integration/run -- --scenario tests/integration/scenarios/chitchat.yaml --run-count 1`

## Notes
- The runtime serves admin/auth surfaces under `/admin`.
- Prompt state is loaded from `prompts.md`.
- Events, state, and modules are persisted in libSQL.
- Decision and routing paths reuse the same event stream rather than a separate thread abstraction.

## Backfill concept embeddings
```bash
cd core-rust
CONCEPT_EMBEDDING_MODEL_DIR=/path/to/model \
MEMGRAPH_URI=bolt://localhost:7687 \
cargo run --bin tsuki-core-rust -- backfill --limit 1000
```

## Backfill conversation recall index
```bash
cd core-rust
MEMGRAPH_PASSWORD=... \
cargo run --bin tsuki-core-rust -- backfill-conversation-recall --limit 1000
```

Production one-shot (snapshot before/after included):
```bash
DOCKER_HOST=ssh://<prod-host> task memgraph/backfill
```
