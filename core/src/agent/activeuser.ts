import { getUserSpecificMCP, MCPAuthHandler, MCPClient } from '../mastra/mcp'
import { ResponseMessage } from './message'
import { ConversationManager } from './conversation'
import { RuntimeContext } from '@mastra/core/runtime-context'
import { UserContext } from './userContext'
import { Responder } from './mastraResponder'
import { MessageRouter } from './router'

export type AgentRuntimeContext = {
  instructions: string
  memory?: string
}

export type MessageChannel = 'websocket' | 'fcm' | 'internal'

export interface MessageInput {
  userId: string
  text: string
  images?: Array<{
    data: string
    mimeType?: string
  }>
}

export interface MessageSender {
  sendMessage(principalUserId: string, message: ResponseMessage): Promise<void>
}

export interface MCPNotificationResourceUpdated {
  uri: string
  title: string
}

export interface MCPNotificationHandler {
  handleSchedulerNotification(userId: string, notification: MCPNotificationResourceUpdated): Promise<void>
}

export class ActiveUser implements UserContext {
  readonly mcp: MCPClient | null = null
  private senders = new Map<MessageChannel, MessageSender>()

  constructor(
    public readonly userId: string,
    public readonly conversation: ConversationManager,
    private responder: Responder,
    private router: MessageRouter,
    private runtimeContext: RuntimeContext<AgentRuntimeContext>,
    private readonly assistantName: string,
    onAuth: MCPAuthHandler | null,
  ) {
    this.mcp = getUserSpecificMCP(userId)

    if (onAuth) {
      // Initialize MCP client requiring authentication
    }

    this.subscribeNotifications().catch((err) => {
      console.error(`Error subscribing to notifications for user ${this.userId}:`, err)
    })
  }

  [Symbol.dispose](): void {
    this.mcp?.[Symbol.dispose]()
  }

  get mcpClient(): MCPClient | null {
    return this.mcp
  }

  async loadMemory(): Promise<string> {
    try {
      const response = await this.mcp?.callTool(
        'structured-memory',
        'read_document',
        {},
      ) as { content: { text: string }[] } | undefined

      return response?.content[0]?.text ?? ''
    } catch (error) {
      console.warn(`Failed to load memory for user ${this.userId}:`, error)
      return ''
    }
  }

  async getCurrentThread(): Promise<string> {
    return this.conversation.currentThread()
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  async getToolsets(): Promise<Record<string, Record<string, any>>> {
    return await this.mcp?.client.getToolsets() ?? {}
  }

  getRuntimeContext(): RuntimeContext<AgentRuntimeContext> {
    return this.runtimeContext
  }

  async processMessage(input: MessageInput): Promise<void> {
    console.log(`AgentService: Processing message for user ${input.userId}:`, input)

    try {
      const decision = await this.router.route(input, this)
      if (decision.action === 'skip') {
        const ackResponse: ResponseMessage = {
          role: 'system',
          user: this.assistantName,
          chat: ['Message received. No response will be sent.'],
          timestamp: Math.floor(Date.now() / 1000),
        }
        await this.sendMessage(ackResponse)
        return
      }

      const response = await this.responder.respond(input, this)
      await this.sendMessage(response)
    } catch (error) {
      console.error('Message processing error:', error)

      const errorResponse: ResponseMessage = {
        role: 'assistant',
        user: this.assistantName,
        chat: ['Internal error!'],
        timestamp: Math.floor(Date.now() / 1000),
      }

      // In any case, a response should be returned
      await this.sendMessage(errorResponse)
    }
  }

  private async subscribeNotifications(): Promise<void> {
    console.log(`ActiveUser: Subscribing to notifications for user ${this.userId}`)

    const mcp = this.mcp
    if (!mcp) {
      console.error('MCP client is not initialized')
      return
    }

    await mcp.client.resources.subscribe('scheduler', 'fired_schedule://recent')
    await mcp.client.resources.onUpdated('scheduler', (params) => {
      console.log(`Received scheduler notification for user ${this.userId}:`, params)
      this.handleSchedulerNotification(params as MCPNotificationResourceUpdated).catch((err) => {
        console.error(`Error handling scheduler notification for user ${this.userId}:`, err)
      })
    })
  }

  public async handleSchedulerNotification(notification: MCPNotificationResourceUpdated): Promise<void> {
    console.log(`AgentService: Handling scheduler notification for user ${this.userId}:`, notification)

    try {
      if (this.responder.handleNotification) {
        const response = await this.responder.handleNotification(notification, this)
        await this.sendMessage(response)
        return
      }

      await this.processMessage({
        userId: 'system',
        text: `Received scheduler notification: ${notification.title}`,
      })
    } catch (err) {
      console.error(`Error handling scheduler notification for user ${this.userId}:`, err)
    }
  }

  public registerMessageSender(channel: MessageChannel, sender: MessageSender, onAuth: MCPAuthHandler | null): void {
    console.log(`ActiveUser: Registering sender for channel ${channel} for user ${this.userId}`)

    this.senders.set(channel, sender)

    if (onAuth && !this.mcp) {
      // Initialize MCP client requiring authentication
      // Subscribe to notifications, if needed
    }
  }

  public deregisterMessageSender(channel: MessageChannel): void {
    console.log(`ActiveUser: Deregistering sender for channel ${channel} for user ${this.userId}`)

    this.senders.delete(channel)
  }

  public async sendMessage(message: ResponseMessage): Promise<void> {
    console.log(`ActiveUser: Sending message to user ${this.userId}:`, message)

    const availableChannels = Array.from(this.senders.keys())
    if (availableChannels.length === 0) {
      console.warn(`No message senders registered for user ${this.userId}. Cannot send message.`)
      return
    }

    for (const [channel, sender] of this.senders.entries()) {
      if (channel === 'fcm' && availableChannels.length >= 2) {
        // Prefer other channels if available
        continue
      }

      try {
        await sender.sendMessage(this.userId, message)
      } catch (error) {
        console.error(`Error sending message to user ${this.userId} via channel ${channel}:`, error)
      }
    }
  }
}
