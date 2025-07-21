import { openai } from '@ai-sdk/openai'
import { Agent } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { weatherTool } from '../tools/weather-tool'

export const tsuki = new Agent({
  name: 'Tsuki',
  instructions: `
    You are a chatting agent.
`,
  model: openai('gpt-4o-mini'),
  tools: { weatherTool },
  memory: new Memory(),
})
