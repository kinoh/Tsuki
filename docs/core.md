# Core System

## Agent

1. **Conversation Management**
   - [`ConversationManager`](core/src/conversation.ts) handles thread IDs (`userId-YYYYMMDD`) and smart continuation logic (previous day's thread continues if updated within 1 hour).
   - Integrates with [`MastraMemory`](core/src/conversation.ts) for persistent message history.

2. **Message Formatting**
   - [`createResponseMessage`](core/src/message.ts) provides unified message format (`ResponseMessage`) for both WebSocket and HTTP APIs.
   - Designed for future multi-modal content, but currently supports text only. MastraMessageV2 and AI SDK UI utilities are supported.

3. **WebSocket & HTTP API**
   - [`WebSocketManager`](core/src/websocket.ts) manages real-time connections, authentication, and message delivery.
   - Modular Express server (`core/src/server/`) exposes REST endpoints for threads, messages, metrics, and metadata.

4. **Encrypted Prompt System**
   - [`src/prompt.ts`](core/src/prompt.ts) loads agent instructions securely using Age encryption and JWK keys.
   - Prompts are mandatory; if missing or invalid, agent startup fails to ensure persona integrity.

5. **MCP Tool Integration**
   - [`mcp`](core/src/mastra/mcp.ts) configures external MCP servers (e.g., structured-memory, RSS).
   - All advanced tools (structured memory, scheduling, etc.) are provided via MCP, with zero built-in tools in core.
   - **Notification**: MCP-based notification/event system is under consideration for future extensibility.

```mermaid
---
config:
  class:
    hideEmptyMembersBox: true
---
classDiagram
    node --> WebSocketManager
    `WebSocket client` --> WebSocketManager : Connect
    MCPClient --> `MCP server` : Get toolsets, Subscribe resources
    AgentService --> ActiveUser : Manage lifecycle
    ActiveUser --> `Agent@mastra/core` : Get response
    ActiveUser --> UsageStorage
    ActiveUser --> ConversationManager
    ActiveUser --> MCPClient
    ActiveUser --> MessageSender : Send responses
    WebSocketManager --> AgentService : Pass messages, register sender
    WebSocketManager ..|> MessageSender
    WebSocketManager --> ClientConnection : For each user
    class `Agent@mastra/core`
    class UsageStorage
    class ConversationManager
    <<interface>> MessageSender
    class WebSocketManager
    class ActiveUser
    class node:::external
    class `MCP server`:::external
    class `WebSocket client`:::external
    classDef external stroke:none,fill:#383838
  ```
