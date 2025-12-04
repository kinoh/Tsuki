import type { Agent, MastraMemory } from '@mastra/core'
import { RuntimeContext } from '@mastra/core/runtime-context'
import { ConversationManager } from './conversation'
import { UsageStorage } from '../storage/usage'
import { loadPromptFromEnv } from './prompt'
import { ActiveUser, AgentRuntimeContext, MessageChannel, MessageInput, MessageSender } from './activeuser'
import { MCPAuthHandler } from '../mastra/mcp'
import { FCMManager } from '../server/fcm'
import { MastraResponder, Responder } from './mastraResponder'
import { AIRouter } from './aiRouter'
import { MessageRouter } from './router'

export async function createAgentService(agent: Agent, memory: MastraMemory, usageStorage: UsageStorage): Promise<AgentService> {
  const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')

  return new AgentService(agent, memory, usageStorage, instructions)
}

export class AgentService {
  private fcm: FCMManager | null = null
  private activeUsers = new Map<string, ActiveUser>()
  private responder: Responder
  private router: MessageRouter

  constructor(
    private agent: Agent,
    private memory: MastraMemory,
    private usageStorage: UsageStorage,
    private commonInstructions: string,
  ) {
    this.responder = new MastraResponder(agent, usageStorage)
    const routerModel = process.env.ROUTER_MODEL ?? 'gpt-4o-mini'
    this.router = new AIRouter(routerModel, commonInstructions)
  }

  start(permanentUsers: string[], fcm?: FCMManager): void {
    this.activeUsers.clear()
    if (fcm) {
      this.fcm = fcm
    }

    for (const userId of permanentUsers) {
      this.activateUser(userId)
    }

    console.log('AgentService started with notification subscription')
  }

  [Symbol.dispose](): void {
    for (const user of this.activeUsers.values()) {
      user[Symbol.dispose]()
    }
    this.activeUsers.clear()
  }

  activateUser(userId: string): ActiveUser {
    const user = this.activeUsers.get(userId)
    if (user) {
      return user
    }

    const conversation = new ConversationManager(this.memory, userId)

    // Create per-user runtime context with common instructions
    // Memory is loaded on-demand in ActiveUser.processMessage
    const userContext = new RuntimeContext<AgentRuntimeContext>()
    userContext.set('instructions', this.commonInstructions)

    const newUser = new ActiveUser(
      userId,
      conversation,
      this.responder,
      this.router,
      userContext,
      this.agent.name,
      null,
    )
    this.activeUsers.set(userId, newUser)

    if (this.fcm) {
      newUser.registerMessageSender('fcm', this.fcm, null)
    }

    return newUser
  }

  async processMessage(userId: string, input: MessageInput): Promise<void> {
    const user = this.activateUser(userId)
    await user.processMessage(input)
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
}
