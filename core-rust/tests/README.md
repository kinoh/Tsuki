# Core Rust test utilities

Scenario-driven WebSocket client and runner for manual E2E verification.
These scripts do not assert behavior; they record logs for human review.

## Usage
Run the full E2E flow (starts `tsuki-core-rust`):
```
cargo run --example test_runner -- tests/client/scenarios/example.yaml
```

Connect to an already running server:
```
cargo run --example test_runner -- --connect tests/client/scenarios/example.yaml
```

Run the client directly (no server startup):
```
cargo run --example ws_scenario -- tests/client/scenarios/example.yaml
```

Format a JSONL log:
```
cargo run --example format_log -- tests/client/logs/20260101-120000.jsonl
```

## Environment variables
Shared by runner and client:
- `WS_URL` (default: `ws://localhost:2953/`)
- `WEB_AUTH_TOKEN` (default: `test-token`)
- `USER_NAME` (default: `test-user`)
- `LOG_DIR` (default: `tests/client/logs`)
- `RESPONSE_TIMEOUT_MS` (default: `60000`)

## Notes
- The runner waits for the WebSocket port to open before executing the scenario.
- Logs are stored as JSONL; each line is a single event (`send`, `receive`, `error`, etc.).
