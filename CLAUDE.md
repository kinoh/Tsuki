# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Tsuki is a kawaii chat agent built with TypeScript/Mastra that provides:
- WebSocket and HTTP API server for real-time communication
- AI-powered chat using Mastra agents with MCP tool integration
- Code execution capabilities via dify-sandbox
- Cross-platform GUI client (desktop and Android) built with Tauri + Svelte
- Intelligent thread management with conversation continuity
- Encrypted prompt system using Age encryption
- Cross-thread semantic recall for persistent memory

## Development Commands

### Main Application (TypeScript/Mastra)
```bash
cd core/
npm install              # Install dependencies
npm start                # Start development server with tsx
npm run start:prod       # Start production server with tsx
npm run build            # No build needed - using tsx in production

# Test WebSocket connection
node scripts/ws_client.js
```

### GUI Client
```bash
cd gui/
npm run dev          # Development server
npm run build        # Build for production
npm run check        # Type checking
npm run tauri dev    # Run Tauri development
npm run tauri build  # Build Tauri app
```

### Docker/Production
```bash
# Using Taskfile (task runner)
task deploy          # Deploy all services
task deploy-core     # Deploy only the core service
task up              # Start services
task down            # Stop services
task build           # Build all images
task build-core      # Build core service
task log-core        # View logs for core service

# Direct docker compose
docker compose up --build --detach

# Check running services
docker ps            # List running containers
docker compose ps    # List compose services

# Execute commands in containers
docker compose exec core npm run build                 # Build in container
docker compose exec core npm test                      # Run tests in container
docker compose exec core npm start                     # Run application in container
docker compose exec core bash                          # Interactive shell in core container

# Service-specific operations
docker compose logs core                                # View core logs
docker compose restart core                            # Restart core service
```


### Prompt Management (Encrypted)
```bash
task decrypt_prompt  # Decrypt prompt file for editing
task encrypt_prompt  # Encrypt prompt file after editing
task diff_prompt     # Compare encrypted vs current

# Manual encryption (from core/ directory)
node --env-file .env scripts/decrypt_prompt.js
node --env-file .env scripts/encrypt_prompt.js
node --env-file .env scripts/generate_key.js  # Generate X25519 key pair
```

## Architecture

### Core Structure
- **Core Application** (`core/src/`): TypeScript/Node.js backend with Mastra
- **GUI Client** (`gui/`): Cross-platform Tauri + Svelte frontend
- **Docker Services**: Microservices for external integrations

### Core Components
- **Application Entry Point** (`core/src/index.ts`): Runtime context setup and server initialization
- **HTTP/WebSocket Server** (`core/src/server.ts`): Unified Express server with WebSocket integration
- **WebSocket Manager** (`core/src/websocket.ts`): Real-time communication handling
- **Conversation Manager** (`core/src/conversation.ts`): Smart thread continuation logic
- **Message Utilities** (`core/src/message.ts`): MastraMessageV2 formatting and content processing
- **Mastra Agent** (`core/src/mastra/agents/tsuki.ts`): AI chat agent with cross-thread semantic memory
- **MCP Integration** (`core/src/mastra/mcp.ts`): External tool integration via RSS MCP server
- **Encrypted Prompts** (`core/src/prompt.ts`): Age encryption for secure prompt storage

### Communication Protocols
- **WebSocket**: Real-time bidirectional communication with authentication
- **HTTP REST API**: Thread management and message history retrieval
- **Unified Message Format**: Consistent ResponseMessage format across interfaces

### Configuration
- Environment variables: `WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, `AGENT_NAME`, `PROMPT_PRIVATE_KEY` (JWK format), `DATA_DIR`
- Configuration files: `conf/default.toml`, `conf/local.toml` (legacy, for GUI client)
- Mastra LibSQL database: `${DATA_DIR}/mastra.db` (default: `./data/mastra.db`)
- RSS MCP server: `${DATA_DIR}/rss_feeds.db` and `${DATA_DIR}/rss_feeds.opml`

### Docker Services
The application runs with multiple services via Docker Compose:
- **core**: TypeScript/Mastra backend with tsx runtime (port 2953)
- **ssrf-proxy**: Secure proxy for dify-sandbox (port 3128, 8194)
- **sandbox**: Code execution environment (dify-sandbox)
- **mumble-server**: Voice chat server (port 64738)
- **voicevox-engine**: Text-to-speech engine (port 50021)

### Tool Integration
The AI agent uses MCP (Model Context Protocol) for tool integration:
- **Zero Built-in Tools**: Core implements no internal tools, complete MCP delegation
- **RSS MCP Server**: External RSS feed management via rss-mcp-lite MCP server
- **MCP-first Strategy**: All external functionality provided via MCP servers
- **Extensible Architecture**: New capabilities added through MCP servers only

### Runtime Environment
- **Development**: tsx with watch mode for hot reload
- **Production**: tsx direct TypeScript execution (no transpilation required)
- **Docker**: Alpine Linux with native build tools for MCP dependencies
- **Unified Runtime**: Consistent tsx-based execution across all environments

### Data Persistence
- **LibSQL Database**: Unified storage for agents, memory, and vector embeddings
- **Cross-thread Semantic Memory**: Resource-scoped semantic recall across conversation sessions
- **Vector Embeddings**: text-embedding-3-small for semantic search with top-5 matches
- **Thread Management**: Daily thread IDs with smart continuation logic
- **Message Storage**: MastraMessageV2 format with unified ResponseMessage interface
- **RSS MCP Data**: Separate database for RSS feed management via MCP server

## Testing

```bash
# Core application tests
cd core/
npm test                 # Run test suite
npm run test:watch       # Run tests in watch mode

# WebSocket testing
node scripts/ws_client.js

# GUI client tests
cd gui/
npm run check            # Type checking
npm run test             # Run tests
```

## Key Files to Understand

- `core/src/index.ts`: Application entry point with runtime context setup
- `core/src/server.ts`: Express HTTP/WebSocket server with unified architecture
- `core/src/websocket.ts`: WebSocket connection management and message processing
- `core/src/conversation.ts`: Thread management with smart continuation logic
- `core/src/message.ts`: MastraMessageV2 formatting and content processing utilities
- `core/src/mastra/agents/tsuki.ts`: Main AI agent with cross-thread semantic memory
- `core/src/mastra/mcp.ts`: MCP client configuration for RSS server integration
- `core/src/prompt.ts`: Age encryption for secure prompt storage
- `gui/src/routes/+page.svelte`: Main GUI interface
- `compose.yaml`: Docker service definitions with tsx runtime
- `Taskfile.yaml`: Development and deployment tasks
- `doc/mastra-backend-implementation.md`: Detailed implementation documentation

## MCP Integration

**Tool Strategy**: The system uses MCP (Model Context Protocol) for extensibility.

**Built-in Capabilities**: 
- Core conversation management
- Message formatting and threading
- Encrypted prompt handling
- Basic agent functionality

**External Tools via MCP**: 
- Scheduling and time management
- Code execution (via dify-sandbox)
- File operations
- API integrations
- Custom business logic

**Benefits**: 
- Clean separation of concerns
- Easy tool discovery and integration
- Standardized tool interfaces
- No core modifications needed for new functionality