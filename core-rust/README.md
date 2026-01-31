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
- `OPENAI_API_KEY` (required)
- `OPENAI_MODEL` (default: gpt-5-mini)
- `LLM_TEMPERATURE` (optional)
- `LLM_MAX_OUTPUT_TOKENS` (optional)

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
- Three fixed prompt-like submodules (curiosity, self_preservation, social_approval) and one decision module call the OpenAI Response API.
- A shared base personality prompt (Japanese) is prepended to all module instructions.
- All outputs (input, submodules, decision, action) are emitted as events.
