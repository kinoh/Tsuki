# Tsuki Rust Core (minimal)

Minimal Rust core for observing the event stream while sending user input over WebSocket.
This uses a shared Event Format and emits every internal step as `type: "event"` messages.

## Run
```
cd core-rust
cargo run
```

Config file:
- `config.toml` (required, no defaults)
- `[[modules]]` defines initial module registry entries
- `llm.temperature_enabled` controls whether temperature is sent (some models reject it)

Environment variables (secrets only):
- `WEB_AUTH_TOKEN` (required)
- `OPENAI_API_KEY` (required)
- `TURSO_AUTH_TOKEN` (required when `db.remote_url` is set)

Environment variables (router concept embedding):
- `CONCEPT_EMBEDDING_MODEL_DIR` (optional; default: `/opt/tsuki/models/quantized-stable-static-embedding-fast-retrieval-mrl-ja`)
  - Required model files: `tokenizer.json`, `model_rest.safetensors`, `embedding.q4_k_m.bin`
  - Startup fails if files are missing or invalid.

## CLI (reuse existing ws_client.js)
```
cd core
WEB_AUTH_TOKEN=test-token WS_URL=ws://localhost:2953/ node scripts/ws_client.js
```

## CLI (Rust example)
```
cd core-rust
WEB_AUTH_TOKEN=test-token WS_URL=ws://localhost:2953/ cargo run --example ws_client
```

## LLM-driven integration test assets
- Integration-test assets are isolated under `tests/integration/`.
- Use task orchestration from `core-rust/Taskfile.yaml`.
- Integration tests use isolated Memgraph in `compose.test.yaml` (`memgraph-test`, `bolt://localhost:7697`).
- Memgraph restore policy for integration setup uses latest snapshot (`integration/memgraph/restore/latest`).
- Harness entrypoint: `cargo run --example integration_harness -- --help`.
- Full run example:
  - `task -t core-rust/Taskfile.yaml integration/run -- --scenario tests/integration/scenarios/chitchat.yaml --run-count 1`

First message is auth: `USER_NAME:WEB_AUTH_TOKEN`.
After auth, send JSON like:
```
{"type":"message","text":"hello"}
```

You will receive event messages:
```
{"type":"event","event":{...}}
```

## Notes
- Three fixed prompt-like submodules (curiosity, self_preservation, social_approval) and one decision module call the OpenAI Response API.
- A shared base personality prompt (Japanese) is prepended to all module instructions.
- Internal state is exposed to the model as three function tools: `state_set`, `state_get`, `state_search`.
- Submodules are registered in a ModuleRegistry (persisted in libSQL).
- Decision uses recent event history from the event store.
- All outputs (input, submodules, decision, action) are emitted as events.
- Events, state, and modules are persisted in libSQL (local when `db.remote_url` is unset).

## Backfill concept embeddings
```
cd core-rust
CONCEPT_EMBEDDING_MODEL_DIR=/path/to/model \
MEMGRAPH_URI=bolt://localhost:7687 \
cargo run --bin tsuki-core-rust -- backfill --limit 1000
```

Production one-shot (snapshot before/after included):
```
DOCKER_HOST=ssh://<prod-host> task memgraph/backfill
```
