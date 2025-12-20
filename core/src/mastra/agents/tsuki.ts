import { openai } from '@ai-sdk/openai'
import { Agent, ToolsInput } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { LibSQLStore, LibSQLVector } from '@mastra/libsql'
import { appLogger } from '../../logger'

export function summon(dataDir: string, openAiModel: string, tools: ToolsInput): Agent {
  appLogger.info(`dataDir: ${dataDir}`, { dataDir })
  appLogger.info(`openAiModel: ${openAiModel}`, { openAiModel })

  const dbPath = `file:${dataDir}/mastra.db`

  return new Agent({
    name: 'Tsuki',
    instructions: ({ runtimeContext }): string => {
      const instructions = runtimeContext.get<string, string>('instructions')
      if (!instructions) {
        appLogger.warn('Instructions not found in runtime context, using default instructions')
        return 'You are a helpful chatting agent.'
      }

      // Append user-specific memory if available
      const memory = runtimeContext.get<string, string>('memory')
      if (memory) {
        return `${instructions}\n\n<memory>\n${memory}\n</memory>`
      }

      return instructions
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
