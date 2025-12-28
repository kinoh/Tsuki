import { getUserSpecificMCP, MCPAuthHandler, MCPClient } from '../mastra/mcp'
import { ResponseMessage, createResponseMessage } from './message'
import { ConversationManager } from './conversation'
import { RequestContext } from '@mastra/core/request-context'
import { UserContext } from './userContext'
import { Responder } from './mastraResponder'
import { MessageRouter } from './router'
import { MastraDBMessage } from '@mastra/core/agent/message-list'
import { ConfigService } from '../configService'
import { appLogger } from '../logger'

export type AgentRuntimeContext = {
  instructions: string
  memory?: string
}

export type MessageChannel = 'websocket' | 'fcm' | 'internal'

export interface MessageInput {
  userId: string
  type?: 'message' | 'sensory'
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
  title?: string
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
    private config: ConfigService,
    private responder: Responder,
    private router: MessageRouter,
    private requestContext: RequestContext<AgentRuntimeContext>,
    private readonly assistantName: string,
    onAuth: MCPAuthHandler | null,
  ) {
    this.mcp = getUserSpecificMCP(config, userId)

    if (onAuth) {
      // Initialize MCP client requiring authentication
    }

    this.subscribeNotifications().catch((err: unknown) => {
      appLogger.error(`Error subscribing to notifications for user ${this.userId}`, { error: err, userId: this.userId })
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
      appLogger.warn(`Failed to load memory for user ${this.userId}`, { error, userId: this.userId })
      return ''
    }
  }

  async getCurrentThread(): Promise<string> {
    return this.conversation.currentThread()
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  async getToolsets(): Promise<Record<string, Record<string, any>>> {
    return await this.mcp?.client.listToolsets() ?? {}
  }

  getRequestContext(): RequestContext<AgentRuntimeContext> {
    return this.requestContext
  }

  async processMessage(input: MessageInput): Promise<void> {
    appLogger.info(`AgentService: Processing message for user ${input.userId}`, { input, userId: input.userId })

    try {
      const decision = await this.router.route(input, this)
      if (decision.action === 'ignore') {
        const ackResponse: ResponseMessage = {
          role: 'system',
          user: this.assistantName,
          chat: ['No response'],
          timestamp: Math.floor(Date.now() / 1000),
        }
        await this.sendMessage(ackResponse)
        return
      }

      const response = await this.responder.respond(input, this)
      await this.sendMessage(response)
    } catch (error) {
      appLogger.error('Message processing error', { error, userId: input.userId })

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

  private routerHistoryLimit(): number {
    const raw = process.env.ROUTER_HISTORY_LIMIT
    const parsed = raw !== undefined ? parseInt(raw, 10) : NaN
    if (Number.isNaN(parsed) || parsed <= 0) {
      return 10
    }
    return parsed
  }

  async getMessageHistory(): Promise<string[]> {
    const limit = this.routerHistoryLimit()
    const messages = await this.conversation.getRecentMessages(limit)
    const formatted = messages.map((message: MastraDBMessage) => createResponseMessage(message, this.assistantName))
    return formatted.map((msg) => {
      const flattened = msg.chat.join(' ').replace(/\s+/g, ' ').trim()
      const truncated = flattened.length > 200 ? `${flattened.slice(0, 200)}…` : flattened
      return `${msg.role}: ${truncated || '[empty]'}`
    })
  }

  private async subscribeNotifications(): Promise<void> {
    appLogger.info(`ActiveUser: Subscribing to notifications for user ${this.userId}`, { userId: this.userId })

    const mcp = this.mcp
    if (!mcp) {
      appLogger.error('MCP client is not initialized', { userId: this.userId })
      return
    }

    await mcp.client.resources.subscribe('scheduler', 'fired_schedule://recent')
    await mcp.client.resources.onUpdated('scheduler', (params) => {
      appLogger.info(`Received scheduler notification for user ${this.userId}`, { params, userId: this.userId })
      this.handleSchedulerNotification(params as MCPNotificationResourceUpdated).catch((err: unknown) => {
        appLogger.error(`Error handling scheduler notification for user ${this.userId}`, { error: err, userId: this.userId })
      })
    })
  }

  public async handleSchedulerNotification(notification: MCPNotificationResourceUpdated): Promise<void> {
    appLogger.info(`AgentService: Handling scheduler notification for user ${this.userId}`, {
      notification,
      userId: this.userId,
    })

    try {
      const rawTitle = notification.title
      let title: string | null = null
      if (typeof rawTitle === 'string') {
        const trimmed = rawTitle.trim()
        if (trimmed.length > 0) {
          title = trimmed
        }
      }

      if (title === null) {
        const resolved = await this.resolveSchedulerNotificationTitle()
        if (resolved === null) {
          appLogger.warn('Scheduler notification title not found', {
            notification,
            userId: this.userId,
          })
          title = 'unknown event'
        } else {
          title = resolved
        }
      }

      const normalizedNotification: MCPNotificationResourceUpdated = {
        ...notification,
        title,
      }

      if (this.responder.handleNotification) {
        const response = await this.responder.handleNotification(normalizedNotification, this)
        await this.sendMessage(response)
        return
      }

      await this.processMessage({
        userId: 'system',
        type: 'message',
        text: `Received scheduler notification: ${title}`,
      })
    } catch (err) {
      appLogger.error(`Error handling scheduler notification for user ${this.userId}`, { error: err, userId: this.userId })
    }
  }

  private async resolveSchedulerNotificationTitle(): Promise<string | null> {
    const mcp = this.mcp
    if (!mcp) {
      return null
    }

    try {
      const response = await mcp.client.resources.read('scheduler', 'fired_schedule://recent') as
        | { contents?: Array<{ text?: string }> }
        | undefined
      const text = response?.contents?.[0]?.text
      if (typeof text !== 'string' || text.trim().length === 0) {
        return null
      }

      const data = JSON.parse(text) as Array<{ message?: string }>
      const last = data.length > 0 ? data[data.length - 1] : null
      if (last !== null && typeof last.message === 'string') {
        const trimmed = last.message.trim()
        if (trimmed.length > 0) {
          return trimmed
        }
      }
    } catch (error) {
      appLogger.warn('Failed to resolve scheduler notification title from fired_schedule resource', {
        error,
        userId: this.userId,
      })
    }

    return null
  }

  public registerMessageSender(channel: MessageChannel, sender: MessageSender, onAuth: MCPAuthHandler | null): void {
    appLogger.info(`ActiveUser: Registering sender for channel ${channel} for user ${this.userId}`, {
      channel,
      userId: this.userId,
    })

    this.senders.set(channel, sender)

    if (onAuth && !this.mcp) {
      // Initialize MCP client requiring authentication
      // Subscribe to notifications, if needed
    }
  }

  public deregisterMessageSender(channel: MessageChannel): void {
    appLogger.info(`ActiveUser: Deregistering sender for channel ${channel} for user ${this.userId}`, {
      channel,
      userId: this.userId,
    })

    this.senders.delete(channel)
  }

  public async sendMessage(message: ResponseMessage): Promise<void> {
    appLogger.info(`ActiveUser: Sending message to user ${this.userId}`, { message, userId: this.userId })

    const availableChannels = Array.from(this.senders.keys())
    if (availableChannels.length === 0) {
      appLogger.warn(`No message senders registered for user ${this.userId}. Cannot send message.`, { userId: this.userId })
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
        appLogger.error(`Error sending message to user ${this.userId} via channel ${channel}`, {
          error,
          userId: this.userId,
          channel,
        })
      }
    }
  }
}
