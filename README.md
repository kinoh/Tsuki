## Tsuki: Kawaii chat agent

### Features

- **TypeScript/Mastra Backend**: Modern AI agent framework with built-in memory
- **WebSocket & HTTP APIs**: Real-time communication and RESTful endpoints
- **Multi-channel Message Delivery**: WebSocket, FCM push notifications, and internal routing
- **Smart Thread Management**: Automatic conversation continuation with timezone-aware daily thread IDs
- **Cross-thread Semantic Recall**: Persistent memory across conversation sessions
- **Per-user Structured Memory**: MCP-based markdown note-taking for personalized context
- **Encrypted Prompts**: Secure agent instruction storage using Age encryption
- **Two-tier MCP Integration**: Universal (RSS) and user-specific (scheduler, structured-memory) tool architecture
- **GUI Client**: Cross-platform desktop and Android app built with [Tauri](https://tauri.app/) + Svelte

### Quick Start

```bash
# Start the backend
cd core/
pnpm install
pnpm start

# Start the GUI client (optional, separate terminal)
cd gui/
npm run dev        # Vite web UI
# For desktop app:
npm run tauri dev

# Or deploy with Docker
task deploy  # or docker compose up --build --detach
```

### Architecture

- **Core Backend** (`core/`): TypeScript/Node.js server with Mastra agents and MCP integration
- **GUI Client** (`gui/`): Tauri + Svelte cross-platform application
- **Docker Deployment**: Docker Compose stack for core, memgraph, sandbox, and voice services

### Documentation

- **[AGENTS.md](AGENTS.md)**: Development guide and current codebase reference
- **[docs/README.md](docs/README.md)**: Design decisions and historical documentation
