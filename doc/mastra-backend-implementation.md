# Mastra Backend Implementation

## Overview

This document describes the Mastra-based backend implementation (`core/` directory) that provides a TypeScript/Node.js chat agent system designed to replace the Rust-based server implementation with WebSocket and HTTP API support.

## Architecture

### Core Components

```
core/
├── src/
│   ├── conversation.ts    # Thread management with smart continuation logic
│   ├── index.ts          # Express server with REST API endpoints
│   ├── message.ts        # Message formatting utilities
│   ├── prompt.ts         # Age encryption for secure prompt loading
│   ├── websocket.ts      # WebSocket server for real-time communication
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

Provides unified message formatting across WebSocket and HTTP interfaces.

**Types:**
```typescript
interface ResponseMessage {
  role: 'user' | 'assistant' | 'system' | 'tool'
  user: string
  chat: string[]
  timestamp: number
}

type MessageContentPart = 
  | { type: 'text', text: string }
  | { type: 'reasoning', text: string }
  | { type: 'tool-call', toolName: string }
  // ... other content types
```

**Functions:**
```typescript
// Convert multi-modal content to plain text
extractTextContent(content: string | MessageContentPart[]): string

// Create unified response message format
createResponseMessage(
  message: MastraMessageV1,
  agentName: string,
  userIdentifier: string
): ResponseMessage
```

**Content Processing:**
- **Text content**: Returned as-is
- **Other types**: Formatted as `[type] content` (e.g., `[tool-call] weatherTool`)
- **Arrays**: Joined with double newlines
- **Multi-modal Content**: Handles text, reasoning, tool calls, files, and images uniformly

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

### 4. HTTP API Server (`src/index.ts`)

Express-based REST API server with thread and message management.

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

#### `POST /messages`
Send message and get agent response (legacy endpoint).

**Features:**
- **Unified Message Format**: All responses use ResponseMessage format
- **Error Handling**: Proper HTTP status codes and error messages
- **Memory Integration**: Uses ConversationManager for thread management

### 5. Mastra Configuration (`src/mastra/`)

Centralized Mastra setup with agents, tools, and workflows.

**Structure:**
```typescript
// mastra/index.ts
export const mastra = new Mastra({
  agents: [tsukiAgent],
  tools: [weatherTool], // Minimal built-in tools
  workflows: [weatherWorkflow],
  // ... configuration
})
```

**Tool Strategy:**
- **Minimal Built-in Tools**: Core implements only essential tools
- **MCP Integration**: External tools provided via MCP (Model Context Protocol)
- **Function Calling**: Replaces Rust-based function calling with MCP-standardized interfaces
- **Extensibility**: New capabilities added through MCP plugins rather than core modifications

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

## Memory Configuration

The Tsuki agent uses resource-scoped semantic recall, enabling the agent to remember and retrieve information from previous conversations across different thread sessions:

```typescript
// core/src/mastra/agents/tsuki.ts
memory: new Memory({
  options: {
    semanticRecall: {
      scope: 'resource', // Enable cross-thread semantic recall
    },
  },
}),
```

This configuration allows the agent to:
- Remember user preferences and information across different conversation sessions
- Maintain context even when switching between different daily threads
- Provide continuity in long-term interactions with users

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
npm start
```

### Build
```bash
npm run build
```

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

The Mastra backend has completely replaced the original Rust-based Tsuki system:

- **Complete Migration**: Core system fully migrated from Rust to TypeScript/Mastra
- **Tool Integration**: Uses MCP (Model Context Protocol) for extensible tool ecosystem
- **Internal Storage**: Mastra handles all database operations internally with built-in memory
- **Simplified Architecture**: Eliminates external vector database dependencies
- **Cross-platform GUI**: Maintained Tauri + Svelte GUI client for desktop and mobile

## Performance Considerations

- **Memory Management**: Uses lazy loading for message history with Mastra's built-in optimization
- **Connection Pooling**: Efficient WebSocket connection management
- **Thread Optimization**: Smart thread continuation reduces memory overhead
- **Message Batching**: Optimized message processing pipeline
- **Simplified Stack**: Eliminates external vector database overhead

## Security

- **Token Authentication**: Required for all WebSocket connections
- **Input Validation**: Validates all user inputs and parameters
- **Error Handling**: Sanitized error messages to prevent information leakage
- **CORS Configuration**: Configurable cross-origin resource sharing

## Future Enhancements

- **Rate Limiting**: Per-user message rate limits
- **Message Persistence**: Optional message encryption
- **Clustering**: Multi-instance support with shared state
- **Metrics**: Performance and usage monitoring
- **MCP Plugin System**: Dynamic MCP server discovery and integration
- **Tool Registry**: MCP-based tool marketplace and management

## Related Documentation

- **[Encrypted Prompt System](./encrypted-prompt-system.md)**: Detailed guide to Age encryption implementation