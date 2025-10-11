import type { Agent as MastraAgent } from '@mastra/core'
import { getUserSpecificMCP, MCPAuthHandler, MCPClient } from '../mastra/mcp'
import { ResponseMessage } from './message'
import { ConversationManager } from './conversation'
import { UsageStorage } from '../storage/usage'
import { RuntimeContext } from '@mastra/core/runtime-context'

export type AgentRuntimeContext = {
  instructions: string
}

export type MessageChannel = 'websocket' | 'fcm' | 'internal'

export interface MessageInput {
  userId: string
  content: string
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

export class ActiveUser {
  private readonly mcp: MCPClient | null = null
  private senders = new Map<MessageChannel, MessageSender>()

  constructor(
    public readonly userId: string,
    public readonly conversation: ConversationManager,
    private agent: MastraAgent,
    private usageStorage: UsageStorage,
    private runtimeContext: RuntimeContext<AgentRuntimeContext>,
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

  async processMessage(input: MessageInput): Promise<void> {
    console.log(`AgentService: Processing message for user ${input.userId}:`, input)

    try {
      const formattedMessage = JSON.stringify({
        modality: 'Text',
        user: input.userId,
        content: input.content,
      })

      const currentThreadId = await this.conversation.currentThread()

      const response = await this.agent.generate(
        [{ role: 'user', content: formattedMessage }],
        {
          memory: {
            resource: this.userId,
            thread: currentThreadId,
            options: {
              lastMessages: 20,
            },
          },
          runtimeContext: this.runtimeContext,
          toolsets: await this.mcp?.client.getToolsets() ?? {},
        },
      )

      console.log(`response id: ${response.response.id}`)
      console.log(`usage: ${JSON.stringify(response.usage)}`)

      await this.usageStorage.recordUsage(
        response,
        currentThreadId,
        this.userId,
        this.agent.name,
      )

      const assistantResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: [response.text],
        timestamp: Math.floor(Date.now() / 1000),
      }

      await this.sendMessage(assistantResponse)

    } catch (error) {
      console.error('Message processing error:', error)

      const errorResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
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

    await this.processMessage({
      userId: 'system',
      content: `Received scheduler notification: ${notification.title}`,
    })
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
