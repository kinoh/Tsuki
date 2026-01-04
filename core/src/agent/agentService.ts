import type { Agent } from '@mastra/core/agent'
import type { MastraMemory } from '@mastra/core/memory'
import { RequestContext } from '@mastra/core/request-context'
import { ConversationManager } from './conversation'
import { UsageStorage } from '../storage/usage'
import { loadPromptFromEnv } from './prompt'
import { ActiveUser, AgentRuntimeContext, MessageChannel, MessageInput, MessageSender } from './activeuser'
import { MCPAuthHandler, MCPClient } from '../mastra/mcp'
import { FCMManager } from '../server/fcm'
import { MastraResponder, Responder } from './mastraResponder'
import { AIRouter } from './aiRouter'
import { ConfigService } from '../configService'
import { logger } from '../logger'

export async function createAgentService(
  config: ConfigService,
  agent: Agent,
  memory: MastraMemory,
  usageStorage: UsageStorage,
  mcp: MCPClient,
): Promise<AgentService> {
  const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')

  return new AgentService(config, agent, memory, usageStorage, instructions, mcp)
}

export class AgentService {
  private fcm: FCMManager | null = null
  private activeUsers = new Map<string, ActiveUser>()
  private responder: Responder

  constructor(
    private config: ConfigService,
    private agent: Agent,
    private memory: MastraMemory,
    private usageStorage: UsageStorage,
    private commonInstructions: string,
    private readonly mcp: MCPClient,
  ) {
    this.responder = new MastraResponder(agent, usageStorage, config)
  }

  start(permanentUsers: string[], fcm?: FCMManager): void {
    this.activeUsers.clear()
    if (fcm) {
      this.fcm = fcm
    }

    for (const userId of permanentUsers) {
      this.activateUser(userId)
    }

    logger.info('AgentService started with notification subscription')
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
    const requestContext = new RequestContext<AgentRuntimeContext>()
    requestContext.set('instructions', this.commonInstructions)

    const routerModel = process.env.ROUTER_MODEL ?? 'gpt-4o-mini'
    const router = new AIRouter(routerModel, this.commonInstructions)

    const newUser = new ActiveUser(
      userId,
      conversation,
      this.config,
      this.responder,
      router,
      requestContext,
      this.agent.name,
      this.mcp,
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
    logger.info({ channel, userId }, 'AgentService: Channel opened')

    const user = this.activateUser(userId)
    user.registerMessageSender(channel, sender, onAuth)
  }

  deregisterMessageSender(userId: string, channel: MessageChannel): void {
    logger.info({ channel, userId }, 'AgentService: Channel closed')

    const user = this.activateUser(userId)
    user.deregisterMessageSender(channel)
  }
}
