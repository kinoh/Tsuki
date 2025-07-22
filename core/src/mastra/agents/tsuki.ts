import { openai } from '@ai-sdk/openai'
import { Agent } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { LibSQLStore, LibSQLVector } from '@mastra/libsql'

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
  memory: new Memory({
    storage: new LibSQLStore({
      url: 'file:./mastra.db',
    }),
    vector: new LibSQLVector({
      connectionUrl: 'file:./mastra.db',
    }),
    embedder: openai.embedding('text-embedding-3-small'),
    options: {
      lastMessages: 20,
      semanticRecall: {
        topK: 5,
        messageRange: 2,
        scope: 'resource', // Enable cross-thread semantic recall
      },
    },
  }),
})
