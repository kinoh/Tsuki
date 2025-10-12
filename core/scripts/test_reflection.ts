import { MastraInstance } from '../src/mastra'
import { summon } from '../src/mastra/agents/tsuki'
import { createAgentService } from '../src/agent/service'
import { InternalMessageSender } from '../src/agent/senders'
import { UsageStorage } from '../src/storage/usage'
import { LogLevel } from '@mastra/loggers'
import { Mastra } from '@mastra/core'
import { LibSQLStore } from '@mastra/libsql'
import fs from 'fs/promises'
import { ActiveUser } from '../src/agent/activeuser'
import { getUniversalMCP } from '../src/mastra/mcp'
import { getClient } from '../src/storage/libsql'
import { ConsoleLogger } from '@mastra/core/logger'

async function doSchenario(user: ActiveUser, receiver: InternalMessageSender, threadIds: string[]): Promise<void> {
  console.log('doSchenario started')

  const readMemory = async (): Promise<string> => {
    const toolResponse = await user.mcpClient?.callTool('structured-memory', 'read_document', {}) as { content: { text: string }[] }
    return toolResponse?.content[0]?.text ?? '(no memory)'
  }

  for (const [index, threadId] of threadIds.entries()) {
    if (index > 0) {
      // Wait for 1 minute to avoid API rate limits
      await new Promise((resolve) => setTimeout(resolve, 60 * 1000))
    }

    user.conversation.fixThread(threadId)

    const memory = await readMemory()

    console.log(`Thread ID: ${threadId}, Current memory: ${memory}`)

    user.processMessage({
      userId: 'system',
      content: `
  <memory>
  ${memory}
  </memory>
  End of conversation; Memorize important conversation points using update_document tool.
  By default, write to the root document.
  Create subdocuments only if a topic becomes too large or complex.
  The memories construct a knowledge base about the user and construct longterm relationship.
  <example>
  - 🎹王道の次を探してた→メトネルやスクリャービンを紹介
  - ✨「つき先生」って呼ばれて照れた
  </example>`,
    })

    void await receiver[Symbol.asyncIterator]().next()
  }

  const finalMemory = await readMemory()
  console.log('Final memory:', finalMemory)

  console.log('doSchenario completed')
}

async function main(): Promise<void> {
  const dataDir = process.env.DATA_DIR ?? './data'
  const tempDataDir = `./data_tmp_${new Date().toISOString().replace(/[^\d]/g, '')}`
  const logLevel = (process.env.LOG_LEVEL as LogLevel) ?? 'debug'

  // Override data directory for testing
  process.env.DATA_DIR = tempDataDir

  console.log(`Using temporary data directory: ${tempDataDir}`)

  await fs.mkdir(tempDataDir, { recursive: true })
  await fs.copyFile(`${dataDir}/mastra.db`, `${tempDataDir}/mastra.db`)

  using mcp = getUniversalMCP()
  const tools = await mcp.client.getTools()

  const tsuki = summon(tempDataDir, process.env.OPENAI_MODEL ?? 'gpt-4o-mini', tools)
  const storage = new LibSQLStore({
    url: `file:${tempDataDir}/mastra.db`,
  })
  const logger = new ConsoleLogger({
    level: logLevel,
  })

  const mastra = new MastraInstance(new Mastra({
    workflows: {},
    agents: { tsuki },
    storage,
    logger,
  }), mcp)

  const agent = mastra.getAgent('tsuki')

  const agentMemory = await agent.getMemory()
  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const usageStorage = new UsageStorage(agentMemory.storage)
  const agentService = await createAgentService(agent, agentMemory, usageStorage)

  const userId = process.env.PERMANENT_USERS
  if (!userId) {
    throw new Error('PERMANENT_USERS environment variable is not set')
  }

  using user = agentService.activateUser(userId)

  const receiver = new InternalMessageSender('multiline')
  user.registerMessageSender('internal', receiver, null)

  await new Promise((resolve) => setTimeout(resolve, 100))

  const threadIds = (await getClient(agentMemory.storage).execute({
    sql: `
    SELECT id FROM mastra_threads
    WHERE resourceId = ?
    ORDER BY createdAt ASC
    LIMIT 3
  `,
    args: [userId],
  })).rows.map(row => row.id as string)

  console.log('Fetched thread IDs:', threadIds)

  await doSchenario(user, receiver, threadIds)
}

main().catch(console.error)
