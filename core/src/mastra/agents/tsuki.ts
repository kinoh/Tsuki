import { openai } from '@ai-sdk/openai'
import { Agent } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { LibSQLStore, LibSQLVector } from '@mastra/libsql'
import { mcp } from '../mcp'

// Use same data directory as main mastra instance
const dataDir = process.env.DATA_DIR ?? './data'
const dbPath = `file:${dataDir}/mastra.db`
const openAiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'

console.log(`dataDir: ${dataDir}`)
console.log(`openAiModel: ${openAiModel}`)

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
  tools: await mcp.getTools(),
})
