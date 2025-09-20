import { mastra } from './mastra/index'
import { serve } from './server/index'
import { ConversationManager } from './agent/conversation'
import { createAgentService } from './agent/service'
import { UsageStorage } from './storage/usage'

// Main function to start server with runtime context
async function main(): Promise<void> {
  const agent = mastra.getAgent('tsuki')

  const agentMemory = await agent.getMemory()
  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const conversation = new ConversationManager(agentMemory)
  const usageStorage = new UsageStorage(agentMemory.storage)
  const agentService = await createAgentService(agent, conversation, usageStorage)

  // Start AgentService (includes MCP subscriptions)
  agentService.start((process.env.PERMANENT_USERS ?? '').split(','))

  await serve(agent, agentService)
}

main().catch(console.error)
