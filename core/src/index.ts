import { RuntimeContext } from '@mastra/core/di'
import { mastra } from './mastra/index'
import { loadPromptFromEnv } from './prompt'
import { serve } from './server'

type AppRuntimeContext = {
  instructions: string
}

// Function to create runtime context with encrypted prompt
async function createRuntimeContext(): Promise<RuntimeContext<AppRuntimeContext>> {
  const runtimeContext = new RuntimeContext<AppRuntimeContext>()
  
  try {
    const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
    runtimeContext.set('instructions', instructions)
  } catch (error) {
    console.warn('Failed to load encrypted prompt, using fallback:', error)
    runtimeContext.set('instructions', 'You are a helpful chatting agent.')
  }

  return runtimeContext
}

// Main function to start server with runtime context
async function main(): Promise<void> {
  const agent = mastra.getAgent('tsuki')
  const runtimeContext = await createRuntimeContext()
  
  serve(agent, runtimeContext)
}

main().catch(console.error)
