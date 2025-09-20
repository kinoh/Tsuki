import { MCPClient } from '@mastra/mcp'
import { getUserSpecificMCP, MCPAuthHandler } from '../mastra/mcp'
import { ResponseMessage } from './message'

export type MessageChannel = 'websocket'

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
  private mcp: MCPClient | null = null
  private senders = new Map<MessageChannel, MessageSender>()

  constructor(private userId: string, private notificationHandler: MCPNotificationHandler, onAuth: MCPAuthHandler | null) {
    this.mcp = getUserSpecificMCP(userId)

    if (onAuth) {
      // Initialize MCP client requiring authentication
    }

    this.subscribeNotifications().catch((err) => {
      console.error(`Error subscribing to notifications for user ${this.userId}:`, err)
    })
  }

  get mcpClient(): MCPClient | null {
    return this.mcp
  }

  public registerMessageSender(channel: MessageChannel, sender: MessageSender, onAuth: MCPAuthHandler | null): void {
    console.log(`ActiveUser: Registering sender for channel ${channel} for user ${this.userId}`)

    this.senders.set(channel, sender)

    if (onAuth && !this.mcp) {
      // Initialize MCP client requiring authentication
      // Subscribe to notifications, if needed
    }
  }

  private async subscribeNotifications(): Promise<void> {
    console.log(`ActiveUser: Subscribing to notifications for user ${this.userId}`)

    const mcp = this.mcp
    if (!mcp) {
      console.error('MCP client is not initialized')
      return
    }

    await mcp.resources.subscribe('scheduler', 'fired_schedule://recent')
    await mcp.resources.onUpdated('scheduler', (params) => {
      console.log(`Received scheduler notification for user ${this.userId}:`, params)
      this.notificationHandler.handleSchedulerNotification(this.userId, params as MCPNotificationResourceUpdated).catch((err) => {
        console.error(`Error handling scheduler notification for user ${this.userId}:`, err)
      })
    })
  }

  public deregisterMessageSender(channel: MessageChannel): void {
    console.log(`ActiveUser: Deregistering sender for channel ${channel} for user ${this.userId}`)

    this.senders.delete(channel)
  }

  public async sendMessage(channel: MessageChannel, message: ResponseMessage): Promise<void> {
    console.log(`ActiveUser: Sending message to channel ${channel} for user ${this.userId}:`, message)

    const sender = this.senders.get(channel)
    if (sender) {
      await sender.sendMessage(this.userId, message)
    } else {
      console.warn(`No sender registered for channel: ${channel}`)
    }
  }
}
