import type { Agent } from '@mastra/core'
import type { MCPClient } from '@mastra/mcp'
import { ConversationManager } from './conversation'
import { UsageStorage } from './storage/usage'
import type { AppRuntimeContext } from './server/types'
import type { RuntimeContext } from '@mastra/core/di'
import { type ResponseMessage } from './message'
import { mcp } from './mastra/mcp'

export interface MessageInput {
  userId: string
  content: string
  clientMcp?: MCPClient
}

export interface MessageSender {
  sendMessage(userId: string, message: ResponseMessage): Promise<void>
}

export interface MCPNotification {
  server: string
  resource: string
  data: {
    type?: string
    userId?: string
    taskId?: string
    message?: string
    [key: string]: unknown
  }
}

export class AgentService {
  private messageSenders = new Map<string, MessageSender>()

  constructor(
    private agent: Agent,
    private conversation: ConversationManager,
    private usageStorage: UsageStorage,
    private runtimeContext: RuntimeContext<AppRuntimeContext>,
  ) {}

  async start(): Promise<void> {
    await mcp.resources.subscribe('scheduler', 'fired_schedule://recent')

    console.log('AgentService started with scheduler subscription')
  }

  registerMessageSender(name: string, sender: MessageSender): void {
    this.messageSenders.set(name, sender)
  }

  async processMessage(input: MessageInput): Promise<void> {
    try {
      const formattedMessage = JSON.stringify({
        modality: 'Text',
        user: input.userId,
        content: input.content,
      })

      const currentThreadId = await this.conversation.currentThread(input.userId)

      const response = await this.agent.generate(
        [{ role: 'user', content: formattedMessage }],
        {
          memory: {
            resource: input.userId,
            thread: currentThreadId,
            options: {
              lastMessages: 20,
            },
          },
          runtimeContext: this.runtimeContext,
          toolsets: input.clientMcp 
            ? await input.clientMcp.getToolsets()
            : await mcp.getToolsets(),
        },
      )

      console.log(`response id: ${response.response.id}`)
      console.log(`usage: ${JSON.stringify(response.usage)}`)

      await this.usageStorage.recordUsage(
        response,
        currentThreadId,
        input.userId,
        this.agent.name,
      )

      const assistantResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: [response.text],
        timestamp: Math.floor(Date.now() / 1000),
      }

      // Send to WebSocket sender
      const webSocketSender = this.messageSenders.get('websocket')
      if (webSocketSender) {
        await webSocketSender.sendMessage(input.userId, assistantResponse)
      }

    } catch (error) {
      console.error('Message processing error:', error)

      const errorResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: ['Internal error!'],
        timestamp: Math.floor(Date.now() / 1000),
      }

      // Send error to WebSocket sender
      const webSocketSender = this.messageSenders.get('websocket')
      if (webSocketSender) {
        await webSocketSender.sendMessage(input.userId, errorResponse)
      }
    }
  }

  async handleNotification(notification: MCPNotification): Promise<void> {
    console.log('MCP Notification received:', notification)
    
    // Handle scheduler notifications
    if (notification.server === 'scheduler') {
      await this.handleSchedulerNotification(notification)
    }
  }

  private async handleSchedulerNotification(notification: MCPNotification): Promise<void> {
    try {
      // Example: Task reminder notification
      if (notification.resource.includes('task') && notification.data.type === 'reminder') {
        const { userId, message } = notification.data
        
        if (typeof userId === 'string' && userId.length > 0) {
          const notificationMessage: ResponseMessage = {
            role: 'assistant',
            user: this.agent.name,
            chat: [`📅 Task Reminder: ${message}`],
            timestamp: Math.floor(Date.now() / 1000),
          }

          // Send to WebSocket if user is connected
          const webSocketSender = this.messageSenders.get('websocket')
          if (webSocketSender) {
            await webSocketSender.sendMessage(userId, notificationMessage)
          }
        }
      }
      
      // Handle other scheduler notification types as needed
      console.log(`Processed scheduler notification for resource: ${String(notification.resource)}`)
      
    } catch (error) {
      console.error('Error handling scheduler notification:', error)
    }
  }
}
