import { getUserSpecificMCP, MCPAuthHandler, MCPClient } from '../mastra/mcp'
import { ResponseMessage, createResponseMessage } from './message'
import { ConversationManager } from './conversation'
import { RequestContext } from '@mastra/core/request-context'
import { UserContext } from './userContext'
import { Responder } from './mastraResponder'
import { MessageRouter } from './router'
import { MastraDBMessage } from '@mastra/core/agent/message-list'
import { ConfigService } from '../configService'
import { logger } from '../logger'

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
      logger.error({ err, userId: this.userId }, 'Error subscribing to notifications')
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
    } catch (err) {
      logger.warn({ err, userId: this.userId }, 'Failed to load memory')
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
    logger.info({ input, userId: input.userId }, 'AgentService: Processing message')

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
    } catch (err) {
      logger.error({ err, userId: input.userId }, 'Message processing error')

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
    logger.info({ userId: this.userId }, 'ActiveUser: Subscribing to notifications')

    const mcp = this.mcp
    if (!mcp) {
      logger.error({ userId: this.userId }, 'MCP client is not initialized')
      return
    }

    await mcp.client.resources.subscribe('scheduler', 'fired_schedule://recent')
    await mcp.client.resources.onUpdated('scheduler', (params) => {
      logger.info({ params, userId: this.userId }, 'Received scheduler notification')
      this.handleSchedulerNotification(params as MCPNotificationResourceUpdated).catch((err: unknown) => {
        logger.error({ err, userId: this.userId }, 'Error handling scheduler notification')
      })
    })
  }

  public async handleSchedulerNotification(notification: MCPNotificationResourceUpdated): Promise<void> {
    logger.info(
      {
        notification,
        userId: this.userId,
      },
      'AgentService: Handling scheduler notification',
    )

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
          logger.warn(
            {
              notification,
              userId: this.userId,
            },
            'Scheduler notification title not found',
          )
          title = 'unknown event'
        } else {
          logger.info(
            {
              resolved,
              userId: this.userId,
            },
            'Resolved scheduler notification title',
          )
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
      logger.error({ err, userId: this.userId }, 'Error handling scheduler notification')
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
    } catch (err) {
      logger.warn(
        {
          err,
          userId: this.userId,
        },
        'Failed to resolve scheduler notification title from fired_schedule resource',
      )
    }

    return null
  }

  public registerMessageSender(channel: MessageChannel, sender: MessageSender, onAuth: MCPAuthHandler | null): void {
    logger.info(
      {
        channel,
        userId: this.userId,
      },
      'ActiveUser: Registering sender',
    )

    this.senders.set(channel, sender)

    if (onAuth && !this.mcp) {
      // Initialize MCP client requiring authentication
      // Subscribe to notifications, if needed
    }
  }

  public deregisterMessageSender(channel: MessageChannel): void {
    logger.info(
      {
        channel,
        userId: this.userId,
      },
      'ActiveUser: Deregistering sender',
    )

    this.senders.delete(channel)
  }

  public async sendMessage(message: ResponseMessage): Promise<void> {
    logger.info({ message, userId: this.userId }, 'ActiveUser: Sending message')

    const availableChannels = Array.from(this.senders.keys())
    if (availableChannels.length === 0) {
      logger.warn({ userId: this.userId }, 'No message senders registered. Cannot send message.')
      return
    }

    for (const [channel, sender] of this.senders.entries()) {
      if (channel === 'fcm' && availableChannels.length >= 2) {
        // Prefer other channels if available
        continue
      }

      try {
        await sender.sendMessage(this.userId, message)
      } catch (err) {
        logger.error(
          {
            err,
            userId: this.userId,
            channel,
          },
          'Error sending message',
        )
      }
    }
  }
}
