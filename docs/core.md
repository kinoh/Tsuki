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
    `WebSocket client` --> WebSocketManager : Connect
    MCPClient --> `MCP server` : Get toolsets, Subscribe resources
    AgentService --> `Agent@mastra/core`
    AgentService --> ConversationManager
    AgentService --> ActiveUser
    AgentService --|> NotificationHandler
    ActiveUser --> MCPClient
    ActiveUser --> NotificationHandler
    ActiveUser --> MessageSender
    WebSocketManager --> AgentService : Pass messages
    WebSocketManager --|> MessageSender
    WebSocketManager --> ClientConnection : For each user
    class `Agent@mastra/core`
    class ConversationManager
    <<interface>> NotificationHandler
    <<interface>> MessageSender
    class WebSocketManager
    class ActiveUser
    class `MCP server`:::external
    class `WebSocket client`:::external
    classDef external stroke:none,fill:#383838
  ```
