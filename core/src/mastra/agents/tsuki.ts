import { openai } from '@ai-sdk/openai'
import { Agent, ToolsInput } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { LibSQLStore, LibSQLVector } from '@mastra/libsql'

export function summon(dataDir: string, openAiModel: string, tools: ToolsInput): Agent {
  console.log(`dataDir: ${dataDir}`)
  console.log(`openAiModel: ${openAiModel}`)

  const dbPath = `file:${dataDir}/mastra.db`

  return new Agent({
    name: 'Tsuki',
    instructions: ({ runtimeContext }): string => {
      const instructions = runtimeContext.get('instructions')
      if (instructions === null || instructions === undefined || instructions === '') {
        console.warn('Instructions not found in runtime context, using default instructions')
        return 'You are a helpful chatting agent.'
      }
      return instructions as string
    },
    model: openai(openAiModel),
    memory: new Memory({
      storage: new LibSQLStore({
        url: dbPath,
      }),
      vector: new LibSQLVector({
        connectionUrl: dbPath,
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
    tools,
  })
}
