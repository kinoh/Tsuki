import { IncomingMessage } from 'http'
import { Agent, MastraMessageV1 } from '@mastra/core'
import { WebSocket } from 'ws'
import { ConversationManager } from './conversation'
import { createResponseMessage } from './message'

interface WebSocketClient {
  user: string
}

export class WebSocketManager {
  private clients = new Map<WebSocket, WebSocketClient>()
  private agent: Agent
  private conversation: ConversationManager

  constructor(agent: Agent) {
    this.agent = agent
    const memory = agent.getMemory()
    if (!memory) {
      throw new Error('Agent must have memory configured for WebSocket functionality')
    }
    this.conversation = new ConversationManager(memory)
  }

  handleConnection(ws: WebSocket, req: IncomingMessage): void {
    console.log(`connected: ${req.url}`)

    ws.on('message', (data) => {
      void (async (): Promise<void> => {
        if (typeof data !== 'string') {
          console.warn('WebSocket error: non-string message')
          return
        }
        await this.handleMessage(ws, data)
      })()
    })
    ws.on('close', () => {
      this.clients.delete(ws)
    })
    ws.on('error', (error) => {
      console.error('WebSocket error:', error)
      this.clients.delete(ws)
    })
  }

  private async handleMessage(ws: WebSocket, message: string): Promise<void> {
    const client = this.clients.get(ws)
    if (!client) {
      const [user, token] = message.split(':', 2)
      if (!this.verifyToken(token)) {
        console.log('Invalid auth token')
        ws.close()
        return
      }

      this.clients.set(ws, {
        user,
      })
      console.log(`User authenticated: ${user}`)
      return
    }

    const timestamp = new Date()
    const userMessage: MastraMessageV1 = {
      id: `dummy-${timestamp.getTime()}`,
      content: message,
      role: 'user',
      createdAt: timestamp,
      threadId: await this.conversation.currentThread(client.user),
      resourceId: client.user,
      type: 'text',
    }

    this.sendToClient(ws, client, userMessage)

    await this.processMessage(ws, client, message)
  }

  private async processMessage(ws: WebSocket, client: WebSocketClient, message: string): Promise<void> {
    try {
      const response = await this.agent.generate([
        { role: 'user', content: message },
      ], {
        memory: {
          resource: client.user,
          thread: await this.conversation.currentThread(client.user),
          options: {
            lastMessages: 20,
          },
        },
      })

      console.log(`response id: ${response.response.id}`)
      console.log(`usage: ${JSON.stringify(response.usage)}`)

      const timestamp = new Date()
      const assistantMessage: MastraMessageV1 = {
        id: `dummy-${timestamp.getTime()}`,
        content: response.text,
        role: 'assistant',
        createdAt: timestamp,
        threadId: await this.conversation.currentThread(client.user),
        resourceId: client.user,
        type: 'text',
      }

      this.sendToClient(ws, client, assistantMessage)

    } catch (error) {
      console.error('Message processing error:', error)

      const timestamp = new Date()
      const errorMessage: MastraMessageV1 = {
        id: `dummy-${timestamp.getTime()}`,
        content: 'Internal error!',
        role: 'assistant',
        createdAt: timestamp,
        threadId: await this.conversation.currentThread(client.user),
        resourceId: client.user,
        type: 'text',
      }

      this.sendToClient(ws, client, errorMessage)
    }
  }

  private sendToClient(ws: WebSocket, client: WebSocketClient, message: MastraMessageV1): void {
    const response = createResponseMessage(message, this.agent.name, client.user)

    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(response))
    }
  }

  private verifyToken(token: string): boolean {
    const expectedToken = process.env.WEB_AUTH_TOKEN
    if (expectedToken === null || expectedToken === '') {
      console.error('WEB_AUTH_TOKEN not set, never authorized')
      return false
    }
    return token === expectedToken
  }
}
