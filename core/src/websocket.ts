import { IncomingMessage } from 'http'
import { Agent } from '@mastra/core'
import { RuntimeContext } from '@mastra/core/di'
import { WebSocket } from 'ws'
import { ConversationManager } from './conversation'
import { ResponseMessage } from './message'
import { UsageStorage } from './storage/usage'

interface WebSocketClient {
  user: string
}

export class WebSocketManager {
  private clients = new Map<WebSocket, WebSocketClient>()
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private agent: Agent<string, any, any>
  private conversation: ConversationManager
  private usageStorage: UsageStorage
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private runtimeContext: RuntimeContext<any>

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  constructor(agent: Agent<string, any, any>, runtimeContext: RuntimeContext<any>) {
    this.agent = agent
    this.runtimeContext = runtimeContext
    const memory = agent.getMemory()
    if (!memory) {
      throw new Error('Agent must have memory configured for WebSocket functionality')
    }
    this.conversation = new ConversationManager(memory)

    // Initialize usage storage with shared LibSQL store from mastra
    this.usageStorage = new UsageStorage(memory.storage)
    void this.usageStorage.initTable()
  }

  handleConnection(ws: WebSocket, req: IncomingMessage): void {
    console.log(`connected: ${req.url}`)

    ws.on('message', (data) => {
      void (async (): Promise<void> => {
        let message: string
        if (typeof data === 'string') {
          message = data
        } else if (Buffer.isBuffer(data)) {
          message = data.toString('utf8')
        } else {
          console.warn('WebSocket error: unsupported message type')
          return
        }
        await this.handleMessage(ws, message)
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

    const userResponse: ResponseMessage = {
      role: 'user',
      user: client.user,
      chat: [message],
      timestamp: Math.floor(Date.now() / 1000),
    }

    this.sendToClient(ws, userResponse)

    await this.processMessage(ws, client, message)
  }

  private async processMessage(ws: WebSocket, client: WebSocketClient, message: string): Promise<void> {
    try {
      const formattedMessage = JSON.stringify({
        modality: 'Text',
        user: client.user,
        content: message,
      })

      const currentThreadId = await this.conversation.currentThread(client.user)

      const response = await this.agent.generate([
        { role: 'user', content: formattedMessage },
      ], {
        memory: {
          resource: client.user,
          thread: currentThreadId,
          options: {
            lastMessages: 20,
          },
        },
        runtimeContext: this.runtimeContext,
      })

      console.log(`response id: ${response.response.id}`)
      console.log(`usage: ${JSON.stringify(response.usage)}`)

      await this.usageStorage.recordUsage(
        response,
        currentThreadId,
        client.user,
        this.agent.name,
      )

      const assistantResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: [response.text],
        timestamp: Math.floor(Date.now() / 1000),
      }

      this.sendToClient(ws, assistantResponse)

    } catch (error) {
      console.error('Message processing error:', error)

      const errorResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: ['Internal error!'],
        timestamp: Math.floor(Date.now() / 1000),
      }

      this.sendToClient(ws, errorResponse)
    }
  }

  private sendToClient(ws: WebSocket, response: ResponseMessage): void {
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
