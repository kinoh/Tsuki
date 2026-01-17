import { openai } from '@ai-sdk/openai'
import { Agent, ToolsInput } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { LibSQLStore, LibSQLVector } from '@mastra/libsql'
import { logger } from '../../internal/logger'

export function summon(dataDir: string, openAiModel: string, tools: ToolsInput): Agent {
  logger.info({ dataDir, openAiModel }, 'hello, world')

  const dbPath = `file:${dataDir}/mastra.db`

  return new Agent({
    id: 'tsuki',
    name: 'Tsuki',
    instructions: ({ requestContext }): string => {
      const instructions = requestContext.get<string, string>('instructions')
      if (!instructions) {
        logger.warn('Instructions not found in request context, using default instructions')
        return 'You are a helpful chatting agent.'
      }

      const personality = requestContext.get<string, string>('personality')
      if (!personality) {
        logger.error('No personality, no spirit')
        throw new Error('No personality found')
      }

      return `${personality}\n\n${instructions}\n`
    },
    model: openai(openAiModel),
    memory: new Memory({
      storage: new LibSQLStore({
        id: 'mastra-storage',
        url: dbPath,
      }),
      vector: new LibSQLVector({
        id: 'mastra-vector',
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
