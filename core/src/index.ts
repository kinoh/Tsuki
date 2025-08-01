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

  await serve(agent, runtimeContext)
}

main().catch(console.error)
