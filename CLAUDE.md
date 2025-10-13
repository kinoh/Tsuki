# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Note**: For detailed design decisions and historical context, see `/docs/README.md`.

## Project Overview

Tsuki is a kawaii chat agent built with TypeScript/Mastra that provides:
- WebSocket and HTTP API server for real-time communication
- AI-powered chat using Mastra agents with MCP tool integration
- Multi-channel message delivery (WebSocket, FCM push notifications, internal)
- Cross-platform GUI client (desktop and Android) built with Tauri + Svelte
- Intelligent thread management with conversation continuity
- Encrypted prompt system using Age encryption
- Cross-thread semantic recall for persistent memory
- Per-user structured memory with MCP integration
- Firebase Cloud Messaging (FCM) push notification support

## Development Commands

### Main Application (TypeScript/Mastra)
```bash
cd core/
pnpm install             # Install dependencies
pnpm start               # Start development server with tsx
pnpm run start:prod      # Start production server with tsx
pnpm run build           # No build needed - using tsx in production
pnpm run lint            # Run TypeScript and ESLint checks

# Verification scripts (DO NOT RUN without explicit request - may consume API credits)
pnpm run test:agent      # Test agent conversation flow
node scripts/test_memory.js      # Test memory functionality
tsx scripts/test_reflection.ts   # Test reflection functionality
node scripts/mcp_subscribe.js    # Test MCP subscription
node scripts/ws_client.js        # Test WebSocket connection
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


### MCP Server Management
```bash
task mcp/build                # Build all MCP servers
task mcp/build-scheduler      # Build scheduler MCP server
task mcp/build-structured-memory  # Build structured-memory MCP server
task setup                    # Setup dev environment (pnpm install + MCP build)
```

### Prompt Management (Encrypted)
```bash
task decrypt_prompt  # Decrypt prompt file for editing
task encrypt_prompt  # Encrypt prompt file after editing

# Manual encryption (from core/ directory)
node --env-file .env scripts/decrypt_prompt.js
node --env-file .env scripts/encrypt_prompt.js
node --env-file .env scripts/generate_key.js  # Generate X25519 key pair
```

### Database Management
```bash
task backup          # Backup database to ./backup/ directory
```

## Architecture

### Core Structure
- **Core Application** (`core/src/`): TypeScript/Node.js backend with Mastra
- **GUI Client** (`gui/`): Cross-platform Tauri + Svelte frontend
- **Docker Services**: Microservices for external integrations

### Core Components
- **Application Entry Point** (`core/src/index.ts`): Runtime context setup and server initialization
- **Agent Service Layer** (`core/src/agent/`): Central orchestration and user management
  - `agent/service.ts`: AgentService - manages ActiveUser instances and MCP subscriptions
  - `agent/activeuser.ts`: ActiveUser - per-user state, MCP clients, and message routing
  - `agent/conversation.ts`: ConversationManager - smart thread continuation logic
  - `agent/message.ts`: Message formatting utilities for MastraMessageV2
  - `agent/prompt.ts`: Encrypted prompt handling with Age encryption
  - `agent/senders.ts`: Multi-channel message sender implementations
- **HTTP/WebSocket Server** (`core/src/server/`): Modular Express server with clear separation of concerns
  - `server/index.ts`: Main server integration and startup
  - `server/websocket.ts`: WebSocketManager - real-time communication handling
  - `server/fcm.ts`: FCMManager - Firebase Cloud Messaging integration
  - `server/types.ts`: Shared type definitions
  - `server/middleware/`: Authentication and access control middlewares
  - `server/routes/`: API endpoint handlers (threads, metrics, metadata, notifications)
- **Mastra Integration** (`core/src/mastra/`): AI agent and MCP configuration
  - `mastra/agents/tsuki.ts`: Main AI chat agent with cross-thread semantic memory
  - `mastra/mcp.ts`: Two-tier MCP client configuration (universal + user-specific)
  - `mastra/index.ts`: Mastra instance creation and configuration
- **Storage Layer** (`core/src/storage/`): Data persistence and metrics
  - `storage/usage.ts`: Token usage tracking and Prometheus metrics
  - `storage/fcm.ts`: FCM token storage in LibSQL
  - `storage/libsql.ts`: LibSQL client utilities
- **Admin Interface** (`core/src/admin/`): Web-based management UI
  - `admin/index.ts`: AdminJS configuration and authentication
  - `admin/resources/ThreadResource.ts`: Thread management resource
  - `admin/resources/MessageResource.ts`: Message viewing resource
  - `admin/resources/StructuredMemoryResource.ts`: Per-user memory management

### Communication Protocols
- **WebSocket**: Real-time bidirectional communication with authentication
- **HTTP REST API**: Thread management and message history retrieval
- **FCM Push Notifications**: Firebase Cloud Messaging for mobile/background delivery
- **Admin Web UI**: AdminJS-based thread management interface at `/admin`
- **Metrics API**: Prometheus-compatible metrics endpoint at `/metrics` (localhost only)
- **System Information API**: System metadata at `/metadata`
- **Unified Message Format**: Consistent ResponseMessage format across all channels

### Configuration
- **Core Environment Variables**:
  - `WEB_AUTH_TOKEN`: Authentication token for HTTP API
  - `OPENAI_API_KEY`: OpenAI API key for agent
  - `OPENAI_MODEL`: Model name (e.g., `gpt-4.1`, `gpt-4o`)
  - `AGENT_NAME`: Agent identifier (default: `tsuki`)
  - `PROMPT_PRIVATE_KEY`: Age encryption private key in JWK format
  - `DATA_DIR`: Data directory path (default: `./data`)
  - `TZ`: Timezone for scheduler (default: `Asia/Tokyo`)

- **FCM Configuration** (optional):
  - `GCP_SERVICE_ACCOUNT_KEY`: Firebase service account credentials (JSON)
  - `FCM_PROJECT_ID`: Firebase project ID

- **Advanced Configuration**:
  - `PERMANENT_USERS`: Comma-separated list of always-active users
  - `ADMIN_JS_TMP_DIR`: AdminJS temporary directory (default: `/tmp/.adminjs`)

- **Data Storage**:
  - Mastra LibSQL database: `${DATA_DIR}/mastra.db`
  - RSS MCP data: `${DATA_DIR}/rss_feeds.db` and `${DATA_DIR}/rss_feeds.opml`
  - User-specific MCP data: `${DATA_DIR}/${userId}__structured_memory/`, `${DATA_DIR}/${userId}__scheduler/`

### Docker Services
The application runs as a single containerized service:
- **core**: TypeScript/Mastra backend with tsx runtime (port 2953)
  - Includes HTTP API server, WebSocket server, and AdminJS UI
  - Built-in MCP servers: scheduler and structured-memory (Rust binaries)
  - External MCP integration: rss-mcp-lite (npm package)

### Tool Integration
The AI agent uses a two-tier MCP (Model Context Protocol) architecture:

- **Universal MCP** (shared across all users via `getUniversalMCP()`):
  - **RSS Feed Management**: rss-mcp-lite npm package
  - Provides shared functionality accessible to all users
  - Single instance per application

- **User-specific MCP** (per-user instances via `getUserSpecificMCP(userId)`):
  - **scheduler**: Time-based notifications and reminders (Rust MCP server)
  - **structured-memory**: Per-user markdown-based note-taking (Rust MCP server)
  - Each user gets isolated MCP client with private data directory
  - Supports MCP resource subscriptions for real-time notifications

- **Architecture Benefits**:
  - Zero built-in tools - complete MCP delegation
  - Clean separation between shared and private functionality
  - MCP-first strategy for extensibility
  - New capabilities added through MCP servers only

### Runtime Environment
- **Development**: tsx with watch mode for hot reload
- **Production**: tsx direct TypeScript execution (no transpilation required)
- **Docker**: Alpine Linux with native build tools for MCP dependencies
- **Unified Runtime**: Consistent tsx-based execution across all environments

### Data Persistence
- **LibSQL Database**: Unified storage for agents, memory, and vector embeddings (`${DATA_DIR}/mastra.db`)
- **Cross-thread Semantic Memory**: Resource-scoped semantic recall across conversation sessions
- **Vector Embeddings**: text-embedding-3-small for semantic search with top-5 matches
- **Thread Management**: Timezone-aware daily thread IDs with 4-hour offset and smart continuation logic
- **Message Storage**: MastraMessageV2 format with unified ResponseMessage interface
- **Usage Metrics Storage**: Token usage tracking with LibSQL persistence
- **Per-user Structured Memory**: MCP-based markdown documents stored per user (`${DATA_DIR}/${userId}__structured_memory/`)
- **Per-user Scheduler Data**: MCP-based time-based notifications (`${DATA_DIR}/${userId}__scheduler/`)
- **FCM Token Storage**: Firebase Cloud Messaging tokens in LibSQL
- **RSS MCP Data**: Shared RSS feed database (`${DATA_DIR}/rss_feeds.db` and OPML file)

## Testing

**Note**: The project currently uses manual verification scripts only. There is no automated regression test suite.

### Manual Verification Scripts
```bash
cd core/

# Static analysis
pnpm run lint            # TypeScript type checking and ESLint validation

# Manual verification (DO NOT RUN without explicit request - consumes API credits)
pnpm run test:agent              # Agent conversation flow testing
node scripts/test_memory.js      # Memory functionality verification
tsx scripts/test_reflection.ts   # Reflection feature testing
node scripts/mcp_subscribe.js    # MCP subscription testing
node scripts/ws_client.js        # WebSocket connection testing
```

### Test Configuration Files
- `scripts/test_agent.yaml`: Test conversation scenarios for agent validation
- `scripts/test_memory.yaml`: Memory test scenarios

### Future Work
Automated unit tests and integration tests are needed for regression testing.

## Key Files to Understand

### Application Entry Point
- `core/src/index.ts`: Main entry point - creates Mastra instance, AgentService, and starts server

### Agent Service Layer
- `core/src/agent/service.ts`: AgentService - orchestrates ActiveUser instances
- `core/src/agent/activeuser.ts`: ActiveUser - per-user state, MCP clients, message routing, and multi-channel delivery
- `core/src/agent/conversation.ts`: ConversationManager - timezone-aware thread management with continuation logic
- `core/src/agent/message.ts`: Message formatting utilities (ResponseMessage, MastraMessageV2)
- `core/src/agent/prompt.ts`: Encrypted prompt loading with Age encryption
- `core/src/agent/senders.ts`: InternalMessageSender for testing and debugging

### Server Components
- `core/src/server/index.ts`: Express server setup, route registration, and startup
- `core/src/server/websocket.ts`: WebSocketManager - handles WebSocket connections and authentication
- `core/src/server/fcm.ts`: FCMManager - Firebase Cloud Messaging integration
- `core/src/server/types.ts`: Shared type definitions
- `core/src/server/middleware/auth.ts`: HTTP authentication middleware
- `core/src/server/middleware/internal.ts`: Internal network access control

### API Routes
- `core/src/server/routes/threads.ts`: Thread and message history endpoints
- `core/src/server/routes/metrics.ts`: Prometheus-compatible usage metrics
- `core/src/server/routes/metadata.ts`: System metadata (Git hash, model info, MCP tools)
- `core/src/server/routes/notification.ts`: FCM token management and test endpoints

### Mastra & AI Integration
- `core/src/mastra/index.ts`: Mastra instance creation with LibSQL storage
- `core/src/mastra/agents/tsuki.ts`: Main AI agent definition with cross-thread semantic memory
- `core/src/mastra/mcp.ts`: Two-tier MCP client configuration (universal + user-specific)

### Storage & Persistence
- `core/src/storage/libsql.ts`: LibSQL client utilities
- `core/src/storage/usage.ts`: Token usage tracking and metrics storage
- `core/src/storage/fcm.ts`: FCM token storage in LibSQL

### Admin Interface
- `core/src/admin/index.ts`: AdminJS configuration and authentication
- `core/src/admin/resources/ThreadResource.ts`: Thread management UI
- `core/src/admin/resources/MessageResource.ts`: Message viewing UI
- `core/src/admin/resources/StructuredMemoryResource.ts`: Per-user memory management UI

### Infrastructure & Configuration
- `compose.yaml`: Docker service definition (single core service)
- `Taskfile.yaml`: Development and deployment task automation
- `docker/core/Dockerfile`: Multi-stage Docker build with Rust MCP servers

### Verification Scripts
- `core/scripts/test_agent.js`: Agent conversation flow testing
- `core/scripts/test_memory.js`: Memory functionality verification
- `core/scripts/test_reflection.ts`: Reflection feature testing
- `core/scripts/mcp_subscribe.js`: MCP subscription testing
- `core/scripts/ws_client.js`: WebSocket connection testing
- `core/scripts/test_agent.yaml`: Agent test scenarios
- `core/scripts/test_memory.yaml`: Memory test scenarios

### Encryption & Security
- `core/scripts/encrypt_prompt.js`: Encrypt system prompt with Age
- `core/scripts/decrypt_prompt.js`: Decrypt system prompt for editing
- `core/scripts/generate_key.js`: Generate X25519 key pair for Age encryption
- `core/src/prompts/initial.txt.encrypted`: Encrypted system prompt

## HTTP API Endpoints

### Core APIs
- `GET /threads` - List all available threads
- `GET /threads/:threadId/messages` - Retrieve message history for a thread
- `GET /metadata` - System information (Git hash, OpenAI model, MCP tools)

### Push Notification Management
- `PUT /notifications/token` - Register FCM token for push notifications
  - Body: `{ "token": "fcm_token_string" }`
  - Requires authentication
- `DELETE /notifications/token` - Unregister FCM token
  - Body: `{ "token": "fcm_token_string" }`
  - Requires authentication
- `GET /notifications/tokens` - List user's registered FCM tokens
  - Requires authentication
- `POST /notifications/test` - Send test notification to user
  - Requires authentication
  - Useful for verifying FCM configuration

### Admin Interface
- `GET /admin` - AdminJS web interface for thread management
  - Authentication required using `WEB_AUTH_TOKEN`
  - Thread viewing, filtering, and deletion capabilities
  - Message history browsing
  - Structured memory management per user

### Monitoring & Metrics
- `GET /metrics` - Prometheus-compatible metrics (localhost access only)
  - Token usage statistics
  - Message counts per thread/user
  - System performance metrics

## Multi-channel Message Delivery

The system supports three message delivery channels, managed by ActiveUser:

### Channel Types
- **WebSocket** (`websocket`): Real-time bidirectional communication
  - Managed by WebSocketManager
  - Requires authentication via Authorization header
  - Primary channel for interactive sessions

- **FCM Push Notifications** (`fcm`): Background/mobile delivery
  - Managed by FCMManager
  - Requires Firebase Cloud Messaging configuration
  - Used as fallback when WebSocket unavailable
  - Automatically avoided when other channels are available

- **Internal** (`internal`): Console output for testing
  - InternalMessageSender for debugging and verification
  - Used by test scripts and manual verification

### Message Routing
- Each ActiveUser can have multiple senders registered
- Messages are sent to all registered channels
- FCM is skipped if 2+ channels are available (preference for real-time channels)
- All channels use unified ResponseMessage format

## Per-user MCP Architecture

Each user gets isolated MCP client instances for privacy and security:

### User-specific MCP Servers
- **scheduler**: Per-user time-based notifications
  - Data stored in `${DATA_DIR}/${userId}__scheduler/`
  - Supports MCP resource subscriptions
  - Real-time notification delivery via MCP protocol

- **structured-memory**: Per-user markdown notes
  - Data stored in `${DATA_DIR}/${userId}__structured_memory/`
  - Accessible via AdminJS UI and MCP tools
  - Injected into system prompt for personalization

### Universal MCP Servers
- **RSS**: Shared RSS feed management
  - Single instance for all users
  - Data stored in `${DATA_DIR}/rss_feeds.db`

### Benefits
- Complete data isolation between users
- MCP subscription support for real-time updates
- Clean separation of shared vs. private functionality

## Structured Memory System

Per-user memory is managed through both MCP and AdminJS:

### MCP Integration
- Agent can read/write user memory via `structured-memory` MCP server
- Memory loaded before each agent invocation
- Injected into runtime context for prompt personalization

### AdminJS Management
- Web UI for viewing/editing user memory at `/admin`
- StructuredMemoryResource provides CRUD operations
- Useful for debugging and manual memory management

### Memory Loading Flow
1. User sends message to agent
2. ActiveUser loads structured memory via MCP
3. Memory injected into agent's runtime context
4. Agent generates response with personalized context
5. Agent can update memory via MCP tools if needed