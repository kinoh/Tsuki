import { WebSocket, type RawData as WebSocketData } from 'ws'
import type { IncomingMessage } from 'node:http'
import { AgentService } from '../agent/service'
import type { ResponseMessage } from '../agent/message'
import { MessageSender } from '../agent/activeuser'
import { clientMessageSchema } from '../shared/wsSchema'

class ClientConnection {
  constructor(
    public readonly user: string,
  ) {}
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
      this.goodbyeClient(ws)
    })

    ws.on('error', (error) => {
      console.error(`WebSocket error for ${user}:`, error)
      this.goodbyeClient(ws)
    })

    ws.on('open', () => {
      console.log(`WebSocket connected: ${user}`)
    })
  }

  private async handleMessage(ws: WebSocket, data: WebSocketData): Promise<void> {
    let client = this.clients.get(ws)
    const message = String(data as Buffer)

    if (!client) {
      // First message should be authentication
      const authResult = this.handleAuth(message)
      if (authResult) {
        client = authResult
        this.acceptClient(ws, client)
        return
      } else {
        ws.close(1002, 'Authentication failed')
        return
      }
    }

    // Process message through AgentService
    try {
      const parsed = clientMessageSchema.parse(JSON.parse(message))

      if (parsed.type === 'message') {
        await this.agentService.processMessage(client.user, {
          userId: client.user,
          type: 'message',
          text: parsed.text ?? '',
          images: parsed.images?.map((image) => ({
            data: image.data,
            mimeType: image.mimeType,
          })),
        })
      } else {
        await this.agentService.processMessage(client.user, {
          userId: client.user,
          type: 'sensory',
          text: parsed.text,
        })
      }
    } catch (err) {
      console.error('WebSocket message parsing error:', err)
      await this.sendMessage(client.user, {
        role: 'system',
        user: 'system',
        chat: ['Invalid message payload'],
        timestamp: Math.floor(Date.now() / 1000),
      })
    }
  }

  private acceptClient(ws: WebSocket, connection: ClientConnection): void {
    this.clients.set(ws, connection)
    this.agentService.registerMessageSender(connection.user, 'websocket', this, this.handleMCPAuth.bind(this))
  }

  private goodbyeClient(ws: WebSocket): void {
    const client = this.clients.get(ws)
    if (client) {
      this.clients.delete(ws)
      this.agentService.deregisterMessageSender(client.user, 'websocket')
    }
  }

  private handleAuth(data: string): ClientConnection | null {
    try {
      const [user, token] = data.split(':', 2)
      
      if (!user || !token) {
        console.log(`Invalid auth attempt: broken message: ${data}`)
        return null
      }

      if (!this.verifyToken(user, token)) {
        console.log(`Invalid auth attempt: invalid token for user: ${user}`)
        return null
      }

      return new ClientConnection(user)
    } catch (error) {
      console.error('Auth parsing error:', error)
      return null
    }
  }

  private verifyToken(_user: string, token: string): boolean {
    const expectedToken = process.env.WEB_AUTH_TOKEN
    if (expectedToken === null) {
      return false
    }
    // Assume the only user with the correct token
    return token === expectedToken
  }

  private async handleMCPAuth(userId: string, server: string, url: string): Promise<void> {
    await this.sendMessage(userId, {
      role: 'user',
      user: '',
      chat: [`Please authenticate with ${server} at ${url}`],
      timestamp: Math.floor(Date.now() / 1000),
    })
  }

  sendMessage(principalUserId: string, message: ResponseMessage): Promise<void> {
    // Find the client connection for this user
    const clientEntry = Array.from(this.clients.entries())
      .find(([, client]) => client.user === principalUserId)

    if (clientEntry) {
      const [ws] = clientEntry
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(JSON.stringify(message))
      }
    }
    
    return Promise.resolve()
  }
}
