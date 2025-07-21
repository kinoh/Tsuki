import { openai } from '@ai-sdk/openai'
import { Agent } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'

export const tsuki = new Agent({
  name: 'Tsuki',
  instructions: `
    You are a chatting agent.
`,
  model: openai('gpt-4o-mini'),
  memory: new Memory(),
})
