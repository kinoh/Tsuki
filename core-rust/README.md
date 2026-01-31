# Tsuki Rust Core (minimal)

Minimal Rust core for observing the event stream while sending user input over WebSocket.
This uses a shared Event Format and emits every internal step as `type: "event"` messages.

## Run
```
cd core-rust
cargo run
```

Environment variables:
- `PORT` (default: 2953)
- `WEB_AUTH_TOKEN` (default: test-token)

## CLI (reuse existing ws_client.js)
```
cd core
WEB_AUTH_TOKEN=test-token WS_URL=ws://localhost:2953/ node scripts/ws_client.js
```

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
- Two fixed prompt-like submodules and one decision module are simulated.
- All outputs (input, submodules, decision, action) are emitted as events.
