# Mastra Backend Implementation

## Overview

This document describes the Mastra-based backend implementation (`core/` directory) that provides a TypeScript/Node.js chat agent system designed to replace the Rust-based server implementation with WebSocket and HTTP API support.

## Architecture

### Core Components

```
core/
├── src/
│   ├── conversation.ts    # Thread management with smart continuation logic
│   ├── index.ts          # Application entry point and runtime context creation
│   ├── message.ts        # Message formatting utilities (MastraMessageV2 support)
│   ├── prompt.ts         # Age encryption for secure prompt loading
│   ├── websocket.ts      # WebSocket server for real-time communication
│   ├── server/           # Modular Express server architecture
│   │   ├── index.ts      # Main server integration and WebSocket setup
│   │   ├── types.ts      # Shared type definitions
│   │   ├── middleware/   # Authentication and access control
│   │   │   ├── auth.ts   # Authentication middleware
│   │   │   ├── internal.ts # Internal network access control
│   │   │   └── index.ts  # Middleware exports
│   │   └── routes/       # API endpoint handlers
│   │       ├── threads.ts # Thread and message API endpoints
│   │       ├── metrics.ts # Usage metrics API
│   │       ├── metadata.ts # System metadata API
│   │       └── index.ts  # Route setup and exports
│   ├── admin/
│   │   ├── index.ts      # AdminJS web UI setup and authentication
│   │   └── resources/
│   │       └── ThreadResource.ts  # AdminJS thread resource definition
│   ├── storage/
│   │   └── usage.ts      # Usage metrics tracking and Prometheus API
│   ├── mastra/
│   │   ├── index.ts      # Mastra configuration and agent setup
│   │   ├── agents/       # Agent definitions
│   │   ├── tools/        # Tool implementations
│   │   └── workflows/    # Workflow definitions
│   └── prompts/
│       └── initial.txt.encrypted  # Encrypted agent instructions
├── scripts/
│   ├── encrypt_prompt.js  # Age encryption script
│   ├── decrypt_prompt.js  # Age decryption script
│   ├── generate_key.js    # X25519 key pair generation
│   └── ws_client.js       # WebSocket test client
```

### Key Features

- **Multi-protocol Communication**: WebSocket and HTTP REST APIs
- **Smart Thread Management**: Automatic thread continuation based on recent activity
- **Unified Message Format**: Consistent response format across all interfaces
- **Memory Management**: Persistent conversation history using Mastra Memory with cross-thread semantic recall
- **Encrypted Prompts**: Secure agent instruction storage using Age encryption
- **MCP-first Tool Strategy**: Minimal built-in tools, leveraging MCP for extensibility
- **AdminJS Web Interface**: Thread management and monitoring web UI
- **Usage Metrics Tracking**: Prometheus-compatible metrics API with LibSQL persistence

## Core Modules

### 1. ConversationManager (`src/conversation.ts`)

Handles intelligent thread management with automatic continuation logic.

**Key Features:**
- **Thread ID Generation**: Creates user-specific thread IDs in format `${userId}-YYYYMMDD`
- **Smart Continuation**: Continues previous day's thread if updated within 1 hour threshold
- **Memory Integration**: Works with MastraMemory for message retrieval

**API:**
```typescript
class ConversationManager {
  constructor(memory: MastraMemory)

  // Get current thread ID for user with smart continuation logic
  async currentThread(userId: string): Promise<string>
}
```

**Thread Continuation Logic:**
1. Generate today's thread ID: `${userId}-YYYYMMDD`
2. Check if previous day's thread exists
3. If exists, get last message timestamp
4. If updated within 1 hour → continue previous thread
5. Otherwise → return today's thread ID

### 2. Message Utilities (`src/message.ts`)

Provides unified message formatting across WebSocket and HTTP interfaces with modernized MastraMessageV2 support.

**Types:**
```typescript
interface ResponseMessage {
  role: 'user' | 'assistant' | 'system' | 'tool'
  user: string
  chat: string[]
  timestamp: number
}

type MessageContentPart = TextUIPart | ReasoningUIPart | ToolInvocationUIPart | 
                         SourceUIPart | FileUIPart | StepStartUIPart
```

**Functions:**
```typescript
// Extract text content from MastraMessageV2 content parts
extractTextContent(content: MastraMessageContentV2): string[]

// Create unified response message format
createResponseMessage(
  message: MastraMessageV2,
  agentName: string,
  userIdentifier: string
): ResponseMessage
```

**Content Processing (MastraMessageV2):**
- **Text parts**: Direct text extraction from `part.text`
- **Reasoning parts**: Combines reasoning text with detail texts
- **Tool invocations**: Formatted as `[tool-invocation] toolName`
- **File parts**: Shows MIME type information
- **Source parts**: Displays source type metadata
- **Multi-modal Content**: Enhanced type safety with exhaustive content type checking
- **UI Parts Integration**: Full compatibility with AI SDK UI utilities

### 3. WebSocket Server (`src/websocket.ts`)

Real-time communication server with authentication and message processing.

**Features:**
- **Token Authentication**: Uses `WEB_AUTH_TOKEN` environment variable
- **User Session Management**: Maps WebSocket connections to authenticated users
- **Real-time Message Processing**: Streams agent responses in real-time

**Protocol:**
1. **Authentication**: Send `userId:token` as first message
2. **Message Exchange**: Send/receive text messages
3. **Response Format**: JSON ResponseMessage objects

**Implementation:**
```typescript
class WebSocketManager {
  constructor(agent: Agent)

  handleConnection(ws: WebSocket, req: IncomingMessage): void
  private async processMessage(ws: WebSocket, client: WebSocketClient, message: string): Promise<void>
}
```

### 4. Application Entry Point (`src/index.ts`)

Clean application entry point responsible for runtime context creation and server initialization.

**Responsibilities:**
- **Runtime Context Creation**: Sets up Mastra runtime with encrypted prompt loading
- **Agent Initialization**: Configures the Tsuki agent with proper context
- **Server Delegation**: Hands off control to the dedicated server module

**Implementation:**
```typescript
async function main(): Promise<void> {
  const agent = mastra.getAgent('tsuki')
  const runtimeContext = await createRuntimeContext()

  serve(agent, runtimeContext)
}
```

### 5. HTTP API Server (`src/server.ts`)

Separated Express-based REST API server with dependency injection and improved type safety.

**Endpoints:**

#### `GET /threads`
List threads for a user.
```json
// Request body
{ "user": "userId" }

// Response
{ "threads": [...] }
```

#### `GET /threads/:id?user=userId`
Get messages from a specific thread in unified ResponseMessage format.
(Thread ID format: `userId-YYYYMMDD`, e.g., `user123-20240115`)
```json
// Response
{ 
  "messages": [
    {
      "role": "user|assistant|system|tool",
      "user": "userId or agentName",
      "chat": ["message content"],
      "timestamp": 1234567890
    }
  ]
}
```

#### `GET /metadata`
Get system information including Git hash, OpenAI model, and available MCP tools.
```json
// Response
{
  "git_hash": "abc123...",
  "openai_model": "gpt-4o-mini",
  "mcp_tools": ["rss.list_feeds", "rss.add_feed", "rss.remove_feed"]
}
```

#### `GET /admin`
AdminJS web interface for thread management (authentication required).

#### `GET /metrics`
Prometheus-compatible metrics endpoint (localhost access only).
```
# HELP tsuki_token_usage_total Total token usage
# TYPE tsuki_token_usage_total counter
tsuki_token_usage_total 12345

# HELP tsuki_messages_total Total messages processed
# TYPE tsuki_messages_total counter
tsuki_messages_total 678

# HELP tsuki_threads_total Total unique threads
# TYPE tsuki_threads_total counter
tsuki_threads_total 90
```

#### `POST /messages`
Send message and get agent response (legacy endpoint).

**Features:**
- **Dependency Injection**: Uses Express.Application.locals for clean dependency management
- **Unified Message Format**: All responses use ResponseMessage format with MastraMessageV2 support
- **Enhanced Type Safety**: Comprehensive TypeScript type definitions and ESLint compliance
- **Error Handling**: Proper HTTP status codes and error messages
- **Memory Integration**: Direct agentMemory integration via app.locals
- **Authentication Middleware**: Centralized user authentication with proper validation

**Architecture Improvements:**
- **Separation of Concerns**: Clean separation between application entry point and server logic
- **Improved Maintainability**: Modular route handlers with proper error handling
- **Type Safety**: Eliminates unsafe type assertions with proper TypeScript definitions
- **Express.Locals Extension**: Proper typing for shared dependencies across routes

### 6. Mastra Configuration (`src/mastra/`)

Modern Mastra setup with LibSQL storage and MCP integration.

**Main Configuration (`mastra/index.ts`):**
```typescript
export const mastra = new Mastra({
  storage: new LibSQLStore({ url: `${dataDir}/mastra.db` }),
  agents: { tsuki },
  tools: {}, // Zero built-in tools - MCP-first strategy
  workflows: {}, // Empty - all logic in MCP servers
  logger: new PinoLogger({ level: 'info' }),
})
```

**Data Management:**
- **Unified Data Directory**: `DATA_DIR` environment variable (default: `./data`)
- **LibSQL Database**: Single database file for all Mastra operations
- **Automatic Directory Creation**: Creates data directory if it doesn't exist

**MCP Integration (`mastra/mcp.ts`):**
```typescript
export const mcp = new MCPClient({
  servers: {
    rss: {
      command: './node_modules/.bin/rss-mcp-lite',
      args: [],
      env: {
        DB_PATH: `${dataDir}/rss_feeds.db`,
        OPML_FILE_PATH: `${dataDir}/rss_feeds.opml`,
      },
    },
  },
})
```

**Agent Configuration (`mastra/agents/tsuki.ts`):**
```typescript
export const tsuki = new Agent({
  name: 'Tsuki',
  model: openai.chat('gpt-4o-mini'),
  instructions: ({ runtimeContext }) => {
    const instructions = runtimeContext.get('instructions')
    return instructions || 'You are a helpful chatting agent.'
  },
  memory: new Memory({
    storage: new LibSQLStore({ url: dbPath }),
    vector: new LibSQLVector({ connectionUrl: dbPath }),
    embedder: openai.embedding('text-embedding-3-small'),
    options: {
      lastMessages: 20,
      semanticRecall: {
        topK: 5,
        messageRange: 2,
        scope: 'resource', // Cross-thread semantic recall
      },
    },
  }),
})
```

**Advanced Memory Features:**
- **Cross-thread Semantic Recall**: Resource-scoped memory across different conversation sessions
- **Vector Embeddings**: text-embedding-3-small for semantic search
- **Message Retention**: Last 20 messages with top-5 semantic matches
- **Unified Storage**: Same LibSQL database for memory and vector data

**Tool Strategy:**
- **Zero Built-in Tools**: Complete MCP delegation for all functionality
- **RSS MCP Server**: External RSS feed management via MCP
- **Extensible Architecture**: New capabilities added through MCP servers only
- **Function Calling**: Rust-based function calling completely replaced with MCP-standardized interfaces

### 7. AdminJS Web Interface (`src/admin/`)

Web-based administration interface for thread management and monitoring.

**Features:**
- **Thread Management**: View, search, and delete conversation threads
- **Authentication**: Protected by `WEB_AUTH_TOKEN`
- **Read-only Safety**: Prevents thread creation/editing to maintain data integrity
- **Custom Resource Definition**: Specialized ThreadResource for MastraMemory integration

**Implementation (`admin/index.ts`):**
```typescript
export function createAdminJS(agentMemory: MastraMemory): AdminJS {
  const admin = new AdminJS({
    resources: [{
      resource: new ThreadResource(agentMemory),
      options: {
        actions: {
          new: { isVisible: false },    // Disable creation
          edit: { isVisible: false },   // Disable editing
          delete: { isVisible: true },  // Enable deletion only
        },
      },
    }],
    rootPath: '/admin',
  })
}
```

**Thread Resource (`admin/resources/ThreadResource.ts`):**
- **Custom AdminJS Resource**: Direct integration with MastraMemory
- **Thread Listing**: Displays thread ID, resource ID, title, and timestamps
- **Safe Operations**: Only allows viewing and deletion of threads
- **Search/Filter**: Built-in search and filtering capabilities

### 8. Usage Metrics System (`src/storage/usage.ts`)

Comprehensive usage tracking and Prometheus-compatible metrics API.

**Features:**
- **Token Usage Tracking**: Records prompt, completion, and total tokens
- **LibSQL Persistence**: Stores metrics in the unified LibSQL database
- **Prometheus Format**: Compatible with standard monitoring tools
- **Performance Optimized**: Indexed queries for efficient metrics retrieval

**Database Schema:**
```sql
CREATE TABLE usage_stats (
  id TEXT PRIMARY KEY,
  timestamp INTEGER NOT NULL,
  thread_id TEXT NOT NULL,
  user_id TEXT NOT NULL,
  agent_name TEXT NOT NULL,
  prompt_tokens INTEGER NOT NULL,
  completion_tokens INTEGER NOT NULL,
  total_tokens INTEGER NOT NULL,
  created_at DATETIME DEFAULT CURRENT_TIMESTAMP
)
```

**API (`UsageStorage`):**
```typescript
class UsageStorage {
  // Record usage for a conversation response
  async recordUsage(response, threadId, userId, agentName): Promise<void>

  // Get aggregated metrics summary
  async getMetricsSummary(): Promise<MetricsSummary>
}
```

**Metrics Output (Prometheus Format):**
```
tsuki_token_usage_total{type="total"} 12345
tsuki_token_usage_total{type="prompt"} 8000
tsuki_token_usage_total{type="completion"} 4345
tsuki_messages_total 678
tsuki_threads_total 90
```

**Security:**
- **Localhost Restriction**: `/metrics` endpoint only accessible from localhost
- **Error Handling**: Graceful fallback to default values on database errors
- **Performance**: Optimized queries with proper indexing

## Environment Configuration

**Required Environment Variables:**
```bash
# Authentication
WEB_AUTH_TOKEN=your-secret-token

# Agent Configuration
AGENT_NAME=tsuki

# Encrypted Prompts (JWK format)
PROMPT_PRIVATE_KEY='{"kty":"OKP","crv":"X25519","d":"...","x":"...","key_ops":["deriveBits"],"ext":true}'
```

**Optional Variables:**
```bash
# API Keys for tools
OPENAI_API_KEY=your-openai-key
# ... other service keys
```

**Note:** Mastra handles database storage internally and doesn't require external database configuration.

## Data Directory Structure

The system uses a unified data directory for all persistent storage:

```
data/                      # Runtime data directory (DATA_DIR env var)
├── mastra.db             # LibSQL database for agents and memory
├── mastra.db-shm         # SQLite shared memory file
├── mastra.db-wal         # SQLite write-ahead log
├── rss_feeds.db          # RSS MCP server database
└── rss_feeds.opml        # RSS feed configuration (OPML format)

# Note: usage_stats table is stored within mastra.db
# Admin interface accesses thread data through MastraMemory API
```

**Configuration:**
- **Environment Variable**: `DATA_DIR` (default: `./data`)
- **Auto-creation**: Directory created automatically if it doesn't exist
- **Docker Support**: Volume mounting supported for persistent storage

## Usage Examples

### WebSocket Client
```javascript
const ws = new WebSocket('ws://localhost:2953')

// Authenticate
ws.send('user123:your-secret-token')

// Send message
ws.send('Hello, how are you?')

// Receive response
ws.onmessage = (event) => {
  const response = JSON.parse(event.data)
  console.log(response.chat[0]) // Agent's response
}
```

### HTTP Client
```javascript
// Get thread messages
const response = await fetch('/threads/user123-20240115?user=user123')
const { messages } = await response.json()

messages.forEach(msg => {
  console.log(`${msg.user}: ${msg.chat[0]}`)
})
```

## Development

### Setup
```bash
cd core/
npm install
npm start                # Development with tsx watch mode
npm run start:prod       # Production with tsx (no transpilation)
```

### Runtime Environment
The application uses **tsx** for both development and production:
- **Development**: tsx with watch mode for hot reload
- **Production**: Direct TypeScript execution without transpilation
- **Docker**: tsx-based runtime in Alpine Linux containers
- **Unified Execution**: Consistent runtime environment across all deployments

### Testing WebSocket
```bash
node scripts/ws_client.js
```

### Encryption Scripts
```bash
# Generate X25519 key pair
node scripts/generate_key.js

# Encrypt prompts (add PROMPT_PRIVATE_KEY to .env first)
node --env-file .env scripts/encrypt_prompt.js

# Decrypt prompts (verification)
node --env-file .env scripts/decrypt_prompt.js
```

## Integration with Main System

The Mastra backend represents a complete architectural modernization:

- **MastraMessageV2 Migration**: Complete migration from V1 to V2 message format with AI SDK UI utilities
- **Separated Server Architecture**: Clean separation of concerns with dedicated server and entry point modules
- **LibSQL Storage**: Unified LibSQL database for all persistent data (agents, memory, vectors)
- **RSS MCP Integration**: External RSS feed management via dedicated MCP server
- **tsx Runtime**: Modern TypeScript execution without build complexity
- **Cross-thread Semantic Memory**: Resource-scoped semantic recall across conversation sessions
- **Zero Built-in Tools**: Complete MCP delegation for all external functionality
- **Cross-platform GUI**: Maintained Tauri + Svelte GUI client for desktop and mobile

## Performance Considerations

- **LibSQL Optimization**: Single database file reduces connection overhead and I/O complexity
- **tsx Runtime**: Eliminates transpilation overhead while maintaining TypeScript safety
- **Cross-thread Semantic Search**: Efficient vector similarity search across conversation sessions
- **Connection Pooling**: Efficient WebSocket connection management
- **Thread Optimization**: Smart thread continuation reduces memory overhead
- **Message Batching**: Optimized message processing pipeline with MastraMessageV2
- **Simplified Stack**: Eliminates external vector database and complex build toolchain

## Security

- **Token Authentication**: Required for all WebSocket connections
- **Input Validation**: Validates all user inputs and parameters
- **Error Handling**: Sanitized error messages to prevent information leakage
- **CORS Configuration**: Configurable cross-origin resource sharing

## Recent Architectural Improvements

### MastraMessageV1 to V2 Migration (c5002c3, 00ac79b, 303e0c4)
- **Complete V2 Support**: Full migration to `MastraMessageV2` and `MastraMessageContentV2`
- **AI SDK Integration**: Native support for AI SDK UI utilities with type safety
- **Enhanced Content Processing**: Support for reasoning, tool-invocation, source, file, and step-start content types
- **Backward Compatibility Removal**: Clean removal of deprecated V1 dependencies

### Server Architecture Separation (6bf7e78)
- **Clean Architecture**: Separation of application entry point (`index.ts`) and server logic (`server.ts`)
- **Dependency Injection**: Proper Express.Application.locals usage for shared dependencies
- **Type Safety**: Comprehensive TypeScript definitions and Express extensions
- **Improved Maintainability**: Modular structure with clear separation of concerns

### tsx Runtime Integration (ed47253)
- **Development**: Hot reload via tsx watch mode
- **Production**: Direct TypeScript execution without transpilation
- **Docker**: Alpine Linux containers with native tsx runtime
- **Unified Environment**: Consistent execution across all deployment scenarios

## Future Enhancements

- **Rate Limiting**: Per-user message rate limits
- **Message Persistence**: Optional message encryption
- **Clustering**: Multi-instance support with shared state
- **Metrics**: Performance and usage monitoring with OpenTelemetry
- **MCP Plugin System**: Dynamic MCP server discovery and integration
- **Tool Registry**: MCP-based tool marketplace and management
- **Vector Search Optimization**: Advanced semantic search with custom embedding models



## Related Documentation

- **[Encrypted Prompt System](./encrypted-prompt-system.md)**: Detailed guide to Age encryption implementation