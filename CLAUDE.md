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
pnpm install             # Install dependencies
pnpm start               # Start development server with tsx
pnpm run start:prod      # Start production server with tsx
pnpm run build           # No build needed - using tsx in production
pnpm run lint            # Run TypeScript and ESLint checks
pnpm run test:agent      # Test tsuki agent with encrypted prompt support

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
docker compose exec core pnpm run build               # Build in container
docker compose exec core pnpm test                    # Run tests in container
docker compose exec core pnpm start                   # Run application in container
docker compose exec core bash                         # Interactive shell in core container

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
- **HTTP/WebSocket Server** (`core/src/server/`): Modular Express server with clear separation of concerns
  - `server/index.ts`: Main server integration and WebSocket setup
  - `server/types.ts`: Shared type definitions
  - `server/middleware/`: Authentication and access control middlewares
  - `server/routes/`: API endpoint handlers grouped by functionality
- **WebSocket Manager** (`core/src/websocket.ts`): Real-time communication handling
- **Conversation Manager** (`core/src/conversation.ts`): Smart thread continuation logic
- **Message Utilities** (`core/src/message.ts`): MastraMessageV2 formatting and content processing
- **Mastra Agent** (`core/src/mastra/agents/tsuki.ts`): AI chat agent with cross-thread semantic memory
- **MCP Integration** (`core/src/mastra/mcp.ts`): External tool integration via RSS MCP server
- **Encrypted Prompts** (`core/src/prompt.ts`): Age encryption for secure prompt storage
- **AdminJS Web UI** (`core/src/admin/index.ts`): Thread management web interface with authentication
- **Usage Metrics** (`core/src/storage/usage.ts`): Token usage tracking and Prometheus metrics

### Communication Protocols
- **WebSocket**: Real-time bidirectional communication with authentication
- **HTTP REST API**: Thread management and message history retrieval
- **Admin Web UI**: AdminJS-based thread management interface at `/admin`
- **Metrics API**: Prometheus-compatible metrics endpoint at `/metrics` (localhost only)
- **System Information API**: System metadata at `/metadata`
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
- **Usage Metrics Storage**: Token usage tracking with LibSQL persistence
- **RSS MCP Data**: Separate database for RSS feed management via MCP server

## Testing

```bash
# Core application tests
cd core/
pnpm test                # Run test suite
pnpm run test:watch      # Run tests in watch mode
pnpm run test:agent      # Test tsuki agent functionality with encrypted prompts
pnpm run lint            # TypeScript type checking and ESLint validation

# WebSocket testing
node scripts/ws_client.js

# GUI client tests
cd gui/
npm run check            # Type checking
npm run test             # Run tests
```

## Key Files to Understand

### Core Application
- `core/src/index.ts`: Application entry point with runtime context setup
- `core/src/websocket.ts`: WebSocket connection management and message processing
- `core/src/conversation.ts`: Thread management with smart continuation logic
- `core/src/message.ts`: MastraMessageV2 formatting and content processing utilities

### Server Architecture (Modular)
- `core/src/server/index.ts`: Main server integration and WebSocket setup
- `core/src/server/types.ts`: Shared type definitions for server components
- `core/src/server/middleware/auth.ts`: Authentication middleware
- `core/src/server/middleware/internal.ts`: Internal network access control
- `core/src/server/routes/threads.ts`: Thread and message API endpoints
- `core/src/server/routes/metrics.ts`: Usage metrics API
- `core/src/server/routes/metadata.ts`: System metadata API

### AI & Integration
- `core/src/mastra/agents/tsuki.ts`: Main AI agent with cross-thread semantic memory
- `core/src/mastra/mcp.ts`: MCP client configuration for RSS server integration
- `core/src/prompt.ts`: Age encryption for secure prompt storage

### Admin & Monitoring
- `core/src/admin/index.ts`: AdminJS web UI for thread management
- `core/src/storage/usage.ts`: Usage metrics tracking and Prometheus API

### Client & Infrastructure
- `gui/src/routes/+page.svelte`: Main GUI interface
- `compose.yaml`: Docker service definitions with tsx runtime
- `Taskfile.yaml`: Development and deployment tasks
- `doc/mastra-backend-implementation.md`: Detailed implementation documentation

### Testing & Quality Assurance
- `core/scripts/test_agent.js`: Standalone agent testing script with encrypted prompt support
- `core/scripts/test_agent.yaml`: Test conversation scenarios for agent validation
- ESLint configuration with TypeScript support for code quality enforcement

## HTTP API Endpoints

### Core APIs
- `GET /threads` - List all available threads
- `GET /threads/:threadId/messages` - Retrieve message history for a thread
- `GET /metadata` - System information (Git hash, OpenAI model, MCP tools)

### Admin Interface
- `GET /admin` - AdminJS web interface for thread management
  - Authentication required using `WEB_AUTH_TOKEN`
  - Thread viewing, filtering, and deletion capabilities
  - Read-only thread management (no creation/editing)

### Monitoring & Metrics
- `GET /metrics` - Prometheus-compatible metrics (localhost access only)
  - Token usage statistics
  - Message counts per thread/user
  - System performance metrics

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

## Server Architecture Details

The server architecture follows a modular design pattern with clear separation of concerns:

### Directory Structure
```
src/server/
├── index.ts              # Main server integration (49 lines)
├── types.ts             # Shared type definitions (27 lines)
├── middleware/
│   ├── auth.ts          # Authentication middleware (36 lines)
│   ├── internal.ts      # IP access control (71 lines)
│   └── index.ts         # Middleware exports (2 lines)
└── routes/
    ├── threads.ts       # Thread/message endpoints (154 lines)
    ├── metrics.ts       # Usage metrics API (23 lines)
    ├── metadata.ts      # System metadata API (38 lines)
    └── index.ts         # Route setup (21 lines)
```

### Design Principles
- **Single Responsibility**: Each file has one clear purpose
- **Dependency Injection**: Dependencies passed via Express app.locals
- **Type Safety**: Shared type definitions prevent inconsistencies
- **Testability**: Independent modules can be tested in isolation
- **Consistent Patterns**: Follows same structure as admin/ module

### Middleware Layer
- **Authentication**: Validates username:token format from Authorization header
- **Internal Access Control**: Restricts certain endpoints to private/local networks
- **Centralized Error Handling**: Consistent error responses across endpoints

### Route Organization
- **threads.ts**: Handles `/threads`, `/threads/:id`, `/messages` endpoints
- **metrics.ts**: Provides `/metrics` endpoint for Prometheus integration
- **metadata.ts**: Serves `/metadata` with system information and Git hash
- **index.ts**: Central route configuration and Express app setup