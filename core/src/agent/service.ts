import type { Agent, MastraMemory } from '@mastra/core'
import { RuntimeContext } from '@mastra/core/di'
import { ConversationManager } from './conversation'
import { UsageStorage } from '../storage/usage'
import { loadPromptFromEnv } from './prompt'
import { ActiveUser, AgentRuntimeContext, MessageChannel, MessageInput, MessageSender } from './activeuser'
import { MCPAuthHandler } from '../mastra/mcp'

export async function createAgentService(agent: Agent, memory: MastraMemory, usageStorage: UsageStorage): Promise<AgentService> {
  const runtimeContext = new RuntimeContext<AgentRuntimeContext>()
  const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
  runtimeContext.set('instructions', instructions)

  return new AgentService(agent, memory, usageStorage, runtimeContext)
}

export class AgentService {
  private activeUsers = new Map<string, ActiveUser>()

  constructor(
    private agent: Agent,
    private memory: MastraMemory,
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

    const conversation = new ConversationManager(this.memory, userId)
    const newUser = new ActiveUser(userId, this.agent, conversation, this.usageStorage, this.runtimeContext, null)
    this.activeUsers.set(userId, newUser)

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
