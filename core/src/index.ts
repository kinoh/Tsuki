import { RuntimeContext } from '@mastra/core/di'
import { mastra } from './mastra/index'
import { loadPromptFromEnv } from './prompt'
import { serve } from './server/index'
import { AppRuntimeContext } from './server/types'

// Function to create runtime context with encrypted prompt
async function createRuntimeContext(): Promise<RuntimeContext<AppRuntimeContext>> {
  const runtimeContext = new RuntimeContext<AppRuntimeContext>()

  const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
  runtimeContext.set('instructions', instructions)

  return runtimeContext
}

// Main function to start server with runtime context
async function main(): Promise<void> {
  const agent = mastra.getAgent('tsuki')
  const runtimeContext = await createRuntimeContext()

  // Create AgentService
  const { AgentService } = await import('./agent-service.js')
  
  const agentMemory = await agent.getMemory()
  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const { ConversationManager } = await import('./conversation.js')
  const { UsageStorage } = await import('./storage/usage.js')
  
  const conversation = new ConversationManager(agentMemory)
  const usageStorage = new UsageStorage(agentMemory.storage)
  
  // Initialize AgentService
  const agentService = new AgentService(agent, conversation, usageStorage, runtimeContext)
  
  // Start AgentService (includes MCP subscriptions)
  await agentService.start()

  await serve(agent, runtimeContext, agentService)
}

main().catch(console.error)
