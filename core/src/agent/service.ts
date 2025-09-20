import type { Agent } from '@mastra/core'
import { RuntimeContext } from '@mastra/core/di'
import { ConversationManager } from './conversation'
import { UsageStorage } from '../storage/usage'
import { type ResponseMessage } from './message'
import { loadPromptFromEnv } from './prompt'
import { ActiveUser, MCPNotificationHandler, MCPNotificationResourceUpdated, MessageChannel, MessageSender } from './activeuser'
import { MCPAuthHandler } from '../mastra/mcp'

type AgentRuntimeContext = {
  instructions: string
}

export interface MessageInput {
  userId: string
  content: string
}

export async function createAgentService(agent: Agent, conversation: ConversationManager, usageStorage: UsageStorage): Promise<AgentService> {
  const runtimeContext = new RuntimeContext<AgentRuntimeContext>()
  const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
  runtimeContext.set('instructions', instructions)

  return new AgentService(agent, conversation, usageStorage, runtimeContext)
}

export class AgentService implements MCPNotificationHandler {
  private activeUsers = new Map<string, ActiveUser>()

  constructor(
    private agent: Agent,
    private conversation: ConversationManager,
    private usageStorage: UsageStorage,
    private runtimeContext: RuntimeContext<AgentRuntimeContext>,
  ) {}

  start(permanentUsers: string[]): void {
    this.activeUsers.clear()

    for (const userId of permanentUsers) {
      this.activateUser(userId)
    }

    console.log('AgentService started with notification subscription')
  }

  activateUser(userId: string): ActiveUser {
    const user = this.activeUsers.get(userId)
    if (user) {
      return user
    }

    const newUser = new ActiveUser(userId, this, null)
    this.activeUsers.set(userId, newUser)

    return newUser
  }

  registerMessageSender(userId: string, channel: MessageChannel, sender: MessageSender, onAuth: MCPAuthHandler | null): void {
    console.log(`AgentService: Channel opened: ${channel} for user ${userId}`)

    const user = this.activateUser(userId)
    user.registerMessageSender(channel, sender, onAuth)
  }

  deregisterMessageSender(userId: string, channel: MessageChannel): void {
    console.log(`AgentService: Channel closed: ${channel} for user ${userId}`)

    const user = this.activateUser(userId)
    user.deregisterMessageSender(channel)
  }

  /**
   * @param principalUserId The ID of the user who has ownership of the conversation (Mastra memory, MCP connection)
   */
  async processMessage(principalUserId: string, input: MessageInput): Promise<void> {
    console.log(`AgentService: Processing message from ${input.userId} (principal: ${principalUserId}): ${input.content}`)

    const activeUser = this.activateUser(principalUserId)

    try {
      const formattedMessage = JSON.stringify({
        modality: 'Text',
        user: input.userId,
        content: input.content,
      })

      const currentThreadId = await this.conversation.currentThread(principalUserId)

      const response = await this.agent.generate(
        [{ role: 'user', content: formattedMessage }],
        {
          memory: {
            resource: principalUserId,
            thread: currentThreadId,
            options: {
              lastMessages: 20,
            },
          },
          runtimeContext: this.runtimeContext,
          toolsets: await activeUser.mcpClient?.getToolsets() ?? {},
        },
      )

      console.log(`response id: ${response.response.id}`)
      console.log(`usage: ${JSON.stringify(response.usage)}`)

      await this.usageStorage.recordUsage(
        response,
        currentThreadId,
        principalUserId,
        this.agent.name,
      )

      const assistantResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: [response.text],
        timestamp: Math.floor(Date.now() / 1000),
      }

      await activeUser.sendMessage('websocket', assistantResponse)

    } catch (error) {
      console.error('Message processing error:', error)

      const errorResponse: ResponseMessage = {
        role: 'assistant',
        user: this.agent.name,
        chat: ['Internal error!'],
        timestamp: Math.floor(Date.now() / 1000),
      }

      // In any case, a response should be returned
      await activeUser.sendMessage('websocket', errorResponse)
    }
  }

  public async handleSchedulerNotification(userId: string, notification: MCPNotificationResourceUpdated): Promise<void> {
    console.log(`AgentService: Handling scheduler notification for user ${userId}:`, notification)

    await this.processMessage(userId, {
      userId: 'system',
      content: `Received scheduler notification: ${notification.title}`,
    })
  }
}
