import { WebSocket, type RawData as WebSocketData } from 'ws'
import type { IncomingMessage } from 'node:http'
import type { MCPClient } from '@mastra/mcp'
import { getDynamicMCP } from './mastra/mcp'
import { AgentService, type MessageSender } from './agent-service'
import type { ResponseMessage } from './message'

class ClientConnection {
  constructor(
    public readonly user: string,
    public readonly mcp: MCPClient,
  ) {}

  async disconnect(): Promise<void> {
    await this.mcp.disconnect()
  }
}

export class WebSocketManager implements MessageSender {
  private clients = new Map<WebSocket, ClientConnection>()
  private readonly agentService: AgentService

  constructor(agentService: AgentService) {
    this.agentService = agentService
  }

  handleConnection(ws: WebSocket, req: IncomingMessage): void {
    const urlParts = new URL(req.url ?? '', 'http://localhost')
    const user = urlParts?.searchParams.get('user') ?? 'anonymous'

    console.log(`New WebSocket connection: ${user}`)

    ws.on('message', (data) => {
      void this.handleMessage(ws, data)
    })

    ws.on('close', () => {
      console.log(`WebSocket disconnected: ${user}`)
      this.clients.delete(ws)
    })

    ws.on('error', (error) => {
      console.error(`WebSocket error for ${user}:`, error)
      this.clients.delete(ws)
    })

    ws.on('open', () => {
      console.log(`WebSocket connected: ${user}`)
    })
  }

  private async handleMessage(ws: WebSocket, data: WebSocketData): Promise<void> {
    let client = this.clients.get(ws)
    const message = String(data as unknown)

    if (!client) {
      // First message should be authentication
      const authResult = this.handleMCPAuth(message)
      if (authResult) {
        client = authResult
        this.clients.set(ws, client)
        return
      } else {
        ws.close(1002, 'Authentication failed')
        return
      }
    }

    // Process message through AgentService
    await this.agentService.processMessage({
      userId: client.user,
      content: message,
      clientMcp: client.mcp,
    })
  }

  private handleMCPAuth(data: string): ClientConnection | null {
    try {
      const authData = JSON.parse(data) as { user: string; token: string }
      const { user, token } = authData

      if (!this.verifyToken(user, token)) {
        return null
      }

      const dynamicMCP = getDynamicMCP(user, (server: string, url: string) => {
        console.log(`MCP Auth for ${user}, server: ${server}, url: ${url}`)
        return Promise.resolve()
      })

      return new ClientConnection(user, dynamicMCP)
    } catch (error) {
      console.error('Auth parsing error:', error)
      return null
    }
  }

  private verifyToken(user: string, token: string): boolean {
    const expectedToken = process.env.WEB_AUTH_TOKEN
    if (expectedToken === null) {
      return false
    }
    return `${user}:${token}` === expectedToken
  }

  // MessageSender interface implementation
  sendMessage(userId: string, message: ResponseMessage): Promise<void> {
    // Find the client connection for this user
    const clientEntry = Array.from(this.clients.entries())
      .find(([, client]) => client.user === userId)
    
    if (clientEntry) {
      const [ws] = clientEntry
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(message))
      }
    }
    
    return Promise.resolve()
  }
}
