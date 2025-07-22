import { openai } from '@ai-sdk/openai'
import { Agent } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'

export const tsuki = new Agent({
  name: 'Tsuki',
  instructions: ({ runtimeContext }): string => {
    const instructions = runtimeContext.get('instructions')
    if (instructions === null || instructions === undefined || instructions === '') {
      console.warn('Instructions not found in runtime context, using default instructions')
      return 'You are a helpful chatting agent.'
    }
    return instructions as string
  },
  model: openai('gpt-4o-mini'),
  memory: new Memory(),
})
