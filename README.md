## Tsuki: Kawaii chat agent

### Features

- **Rust Backend**: Event-stream based runtime in `core-rust/`
- **WebSocket & HTTP APIs**: Real-time chat plus REST endpoints defined in `api-specs/`
- **Multi-channel Message Delivery**: WebSocket, notifications, and internal event routing
- **Prompt File Runtime**: Prompt state stored in `prompts.md` under runtime data
- **MCP-first Architecture**: Runtime capabilities are provided by MCP servers
- **TTS**: VoiceVox synthesis with ja-accent support via `/tts`
- **GUI Client**: Cross-platform desktop and Android app built with [Tauri](https://tauri.app/) + Svelte

### Quick Start

```bash
# Start the backend
cd core-rust/
cargo run

# Optional WebSocket CLI
WEB_AUTH_TOKEN=test-token WS_URL=ws://localhost:2953/ cargo run --example ws_client

# Start the GUI client (optional, separate terminal)
cd gui/
npm run dev
# For desktop app:
npm run tauri dev

# Or deploy with Docker
task deploy
```

### Architecture

- **Backend** (`core-rust/`): Rust runtime, event store, admin/auth surfaces, and MCP integration
- **GUI Client** (`gui/`): Tauri + Svelte cross-platform application
- **Docker Deployment**: Docker Compose stack for runtime, memgraph, sandbox, and voice services

### Documentation

- **[AGENTS.md](AGENTS.md)**: Current codebase reference
- **[docs/README.md](docs/README.md)**: Historical design decisions and change records
- **[api-specs/openapi.yaml](api-specs/openapi.yaml)**: HTTP API contract
- **[api-specs/asyncapi.yaml](api-specs/asyncapi.yaml)**: WebSocket protocol contract
