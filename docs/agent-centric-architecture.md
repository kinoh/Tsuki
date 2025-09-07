# Agent-Centric Architecture Refactoring

## Overview

This document describes the architectural refactoring from a server-centric to an agent-centric design to support MCP (Model Context Protocol) resource subscriptions, specifically for the scheduler MCP server.

## Problem Statement

The previous architecture was server-centric:
- `main()` → `serve()` with server as the primary entry point
- Agent was treated as a passive tool called by the server
- Only Web API (WebSocket/HTTP) interfaces were supported
- No support for MCP server notifications or resource subscriptions
- Scheduler MCP server couldn't send proactive messages to users

## Solution: Agent-Centric Architecture

### Core Design Principles

1. **Agent as Central Orchestrator**: The Agent becomes the primary coordinator handling all message processing
2. **Interface Abstraction**: Multiple interfaces (WebSocket, HTTP, MCP) delegate to the Agent
3. **Dependency Inversion**: Agent depends on abstract `MessageSender` interfaces, not concrete implementations
4. **Event-Driven**: All inputs (WebSocket messages, HTTP requests, MCP notifications) are processed uniformly

### Architecture Components

```
┌─────────────────────────────────────────────────────────┐
│                    AgentService                         │
│  ┌─────────────────┬──────────────────┬─────────────────┐ │
│  │ Message         │ MCP Notification │ Usage & Memory  │ │
│  │ Processing      │ Handling         │ Management      │ │
│  └─────────────────┴──────────────────┴─────────────────┘ │
└─────────────────────────────────────────────────────────┘
           ▲                    ▲                    ▲
           │                    │                    │
     ┌──────────┐         ┌──────────┐         ┌──────────┐
     │WebSocket │         │   HTTP   │         │   MCP    │
     │Interface │         │Interface │         │Interface │
     └──────────┘         └──────────┘         └──────────┘
```

### Key Components

#### AgentService (`core/src/agent-service.ts`)
- **Central message processing**: Unified handling of all message types
- **Interface registration**: Manages `MessageSender` implementations
- **MCP notification handling**: Processes scheduler notifications and routes to appropriate users
- **Autonomous startup**: Self-manages MCP resource subscriptions

#### WebSocketManager (`core/src/websocket.ts`)
- **MessageSender implementation**: Implements `MessageSender` interface for dependency inversion
- **WebSocket protocol handling**: Manages WebSocket connections, authentication, and per-client MCP
- **Message delivery**: Sends responses to connected WebSocket clients
- **Delegation pattern**: Forwards message processing to `AgentService`

### Message Flow

#### WebSocket Message Processing
1. **WebSocket receives message** → `WebSocketManager.handleMessage()`
2. **Delegate to Agent** → `AgentService.processMessage()`
3. **Agent generates response** → Uses Mastra Agent with MCP tools
4. **Response delivery** → `WebSocketManager.sendMessage()` (via MessageSender interface)

#### MCP Notification Handling (NEW)
1. **Scheduler MCP sends notification** → `AgentService.handleNotification()`
2. **Process notification** → Extract user context and generate appropriate message
3. **Route to user** → `WebSocketManager.sendMessage()` (via MessageSender interface) if user connected
4. **Fallback handling** → Store notification for later delivery if user offline

### Benefits

#### Functionality
- **Scheduler MCP Support**: Can now receive and process scheduler notifications (task reminders, schedule updates)
- **Multi-interface Support**: Ready for additional interfaces (HTTP push, mobile notifications, etc.)
- **Unified Processing**: Consistent message handling across all interfaces

#### Architecture
- **Clean Separation**: Clear boundaries between interface handling and business logic  
- **Testability**: AgentService can be tested independently of interface implementations
- **Extensibility**: New interfaces can be added without modifying core logic
- **Maintainability**: Single source of truth for message processing logic

### Implementation Changes

#### New Files
- `core/src/agent-service.ts`: Central orchestrator class

#### Modified Files
- `core/src/index.ts`: Updated initialization to create AgentService first
- `core/src/websocket.ts`: Integrated MessageSender interface and eliminated duplicate connection management
- `core/src/server/index.ts`: Updated to register WebSocketManager as MessageSender
- `core/src/mastra/mcp.ts`: Added scheduler resource subscription support

#### Removed Files
- `core/src/websocket-sender.ts`: Functionality integrated into WebSocketManager to eliminate duplicate connection management

#### Preserved Functionality
- All existing WebSocket and HTTP API functionality maintained
- AdminJS interface unchanged
- Usage tracking and metrics unchanged
- Authentication and security unchanged

## Future Enhancements

### MCP Integration
- Complete scheduler MCP resource subscription implementation
- Add support for other MCP server notifications
- Implement notification queueing for offline users

### Interface Expansion
- HTTP Server-Sent Events (SSE) for real-time web notifications
- Mobile push notifications via AgentService
- Email/SMS notifications for critical scheduler events

### Advanced Features
- Message routing based on user preferences
- Notification filtering and priority handling
- Cross-interface message synchronization

## Migration Notes

This refactoring maintains full backward compatibility:
- Existing WebSocket clients continue to work unchanged
- HTTP API endpoints remain functional
- No configuration changes required
- All existing features preserved

The architecture is now ready to support scheduler MCP resource subscriptions while providing a foundation for future interface additions.