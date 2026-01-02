import { access } from 'fs/promises'
import { MCPClient } from '../src/mastra/mcp'
import { summon } from '../src/mastra/agents/tsuki'
import { RequestContext } from '@mastra/core/request-context'
import type { MastraDBMessage } from '@mastra/core/agent/message-list'
import { createResponseMessage } from '../src/agent/message'

type CliOptions = {
  userId: string
  daysPerChunk: number
  maxChunks?: number
}

type LogEntry = {
  role: 'user' | 'assistant'
  text: string
  createdAt: Date
}

type Chunk = {
  startDay: string
  endDay: string
  messages: LogEntry[]
}

const STEP1_PROMPT = [
  '会話ログを元に内容の整理、既存概念の検索、新規概念・関係・エピソードの作成、感情評価の更新を段階的に行いたい',
  '一つずつ指示したstepを実行していってほしい',
  'Step 1: 整理',
  '- 盛り上がった話題、低調だった話題を分けて列挙',
  '- is-a / part-of / evokes の関係候補を抽出',
  '- 具体的なエピソード候補を抽出',
  '- 繰り返し語られた概念を抽出',
].join('\n')

const STEP2_PROMPT = [
  'Step 2: 検索',
  '- 抽出した概念語を keywords にして concept_search を実行',
  '- 既存の候補（部分一致/高arousal）を確認し、採用する概念名を決める',
].join('\n')

const STEP3_PROMPT = [
  'Step 3: 概念と関係の作成',
  '- 抽出結果を concept graph に反映（concept_upsert / relation_add のみ）',
  '- episode_add はこのステップでは実行しない',
  '- 文章出力は不要',
].join('\n')

const STEP4_PROMPT = [
  'Step 4: エピソード作成',
  '- Step 1 のエピソード候補に基づいて episode_add を実行',
  '- 1エピソード = 1話題にする（まとめすぎない）',
  '- 使う概念は少数に絞る（目安: 3〜7）',
  '- Step 2/3 で決めた概念名に合わせる',
  '- 文章出力は不要',
].join('\n')

const STEP5_PROMPT = [
  'Step 5: update_affect',
  '- valenceの値（-1.0〜1.0）は以下を目安にする',
  '  - -1.0: 強烈な拒否/損失/不快',
  '  - -0.8: かなりの不満/緊張',
  '  - -0.6: 明確な落胆/不快',
  '  - -0.4: 低調/小さな不満',
  '  - -0.2: わずかな違和感/停滞',
  '  -  0.0: 中立/事実整理',
  '  -  0.2: 小さな好意/前向き',
  '  -  0.4: ほどよい満足/前進',
  '  -  0.6: はっきりした好意/盛り上がり',
  '  -  0.8: 強い満足/高揚',
  '  -  1.0: 最高潮/完全な達成感',
  '- Step 3 で作成した概念に対して update_affect を実行',
  '- 文章出力は不要',
].join('\n')

function printUsage(): void {
  console.log('Usage: pnpm tsx core/scripts/backfill_concept_graph.ts --user-id <id> --days-per-chunk <n> [--max-chunks <n>]')
}

function parseArgs(): CliOptions {
  const args = process.argv.slice(2)
  let userId: string | null = null
  let daysPerChunk: number | null = null
  let maxChunks: number | null = null

  for (let i = 0; i < args.length; i += 1) {
    const arg = args[i]
    if (arg === '--user-id') {
      userId = args[i + 1] ?? null
      i += 1
      continue
    }
    if (arg === '--days-per-chunk') {
      const raw = args[i + 1]
      daysPerChunk = raw ? Number(raw) : NaN
      i += 1
      continue
    }
    if (arg === '--max-chunks') {
      const raw = args[i + 1]
      maxChunks = raw ? Number(raw) : NaN
      i += 1
      continue
    }
    if (arg === '--help' || arg === '-h') {
      printUsage()
      process.exit(0)
    }
  }

  if (!userId || userId.trim() === '') {
    console.error('Error: --user-id is required')
    printUsage()
    process.exit(1)
  }
  if (!daysPerChunk || !Number.isFinite(daysPerChunk) || daysPerChunk <= 0) {
    console.error('Error: --days-per-chunk is required and must be a positive number')
    printUsage()
    process.exit(1)
  }
  if (maxChunks !== null && (!Number.isFinite(maxChunks) || maxChunks <= 0)) {
    console.error('Error: --max-chunks must be a positive number')
    printUsage()
    process.exit(1)
  }

  return {
    userId: userId.trim(),
    daysPerChunk,
    maxChunks: maxChunks === null ? undefined : maxChunks,
  }
}

async function loadInstructions(): Promise<string> {
  const { loadPromptFromEnv } = await import('../src/agent/prompt')
  return await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
}

function buildConceptGraphEnv(): Record<string, string> {
  const env: Record<string, string> = {
    TZ: process.env.TZ ?? 'Asia/Tokyo',
  }
  if (process.env.MEMGRAPH_URI) env.MEMGRAPH_URI = process.env.MEMGRAPH_URI
  if (process.env.MEMGRAPH_USER) env.MEMGRAPH_USER = process.env.MEMGRAPH_USER
  if (process.env.MEMGRAPH_PASSWORD) env.MEMGRAPH_PASSWORD = process.env.MEMGRAPH_PASSWORD
  if (process.env.AROUSAL_TAU_MS) env.AROUSAL_TAU_MS = process.env.AROUSAL_TAU_MS
  return env
}

function formatDayKey(date: Date, timeZone: string): string {
  const formatter = new Intl.DateTimeFormat('ja-JP', {
    timeZone,
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
  })
  return formatter.format(date)
}

function buildChunks(entries: LogEntry[], daysPerChunk: number, timeZone: string): Chunk[] {
  const chunks: Chunk[] = []
  let current: Chunk | null = null
  const dayKeys = new Set<string>()

  for (const entry of entries) {
    const dayKey = formatDayKey(entry.createdAt, timeZone)
    const shouldStartNew = current === null
      || (!dayKeys.has(dayKey) && dayKeys.size >= daysPerChunk)

    if (shouldStartNew) {
      if (current) {
        chunks.push(current)
      }
      dayKeys.clear()
      current = {
        startDay: dayKey,
        endDay: dayKey,
        messages: [],
      }
    }

    dayKeys.add(dayKey)
    if (current) {
      current.endDay = dayKey
      current.messages.push(entry)
    }
  }

  if (current) {
    chunks.push(current)
  }

  return chunks
}

async function fetchAllMessages(agentMemory: {
  listThreadsByResourceId: (args: { resourceId: string }) => Promise<{ threads: Array<{ id: string }> }>
  recall: (args: {
    threadId: string
    perPage: number
    page: number
    orderBy: { field: 'createdAt'; direction: 'ASC' | 'DESC' }
  }) => Promise<{ messages: MastraDBMessage[] }>
}, userId: string, agentName: string): Promise<LogEntry[]> {
  const { threads } = await agentMemory.listThreadsByResourceId({ resourceId: userId })

  const userThreads = threads
    .filter((thread) => thread.id.startsWith(`${userId}-`))
    .sort((a, b) => {
      const dateA = a.id.substring(userId.length + 1)
      const dateB = b.id.substring(userId.length + 1)
      return dateA.localeCompare(dateB)
    })

  const entries: LogEntry[] = []

  for (const thread of userThreads) {
    let page = 0
    const perPage = 1000
    while (true) {
      const result = await agentMemory.recall({
        threadId: thread.id,
        perPage,
        page,
        orderBy: { field: 'createdAt', direction: 'ASC' },
      })
      const messages = result.messages ?? []
      if (messages.length === 0) {
        break
      }

      for (const message of messages) {
        if (message.role !== 'user' && message.role !== 'assistant') {
          continue
        }
        const response = createResponseMessage(message, agentName)
        const text = response.chat.join(' ').replace(/\s+/g, ' ').trim()
        if (!text) {
          continue
        }
        entries.push({
          role: message.role,
          text,
          createdAt: message.createdAt,
        })
      }

      if (messages.length < perPage) {
        break
      }
      page += 1
    }
  }

  return entries.sort((a, b) => a.createdAt.getTime() - b.createdAt.getTime())
}

async function runStep(
  agent: { generate: (...args: any[]) => Promise<{ text?: string }> },
  requestContext: RequestContext,
  toolsets: unknown,
  prompt: string,
  stepLabel: string,
): Promise<{ text: string }> {
  const startedAt = Date.now()
  const response = await agent.generate(
    [{ role: 'user', content: prompt }],
    { requestContext, toolsets },
  )
  const durationMs = Date.now() - startedAt
  const text = response.text?.trim() ?? ''
  console.log(`${stepLabel} done (${durationMs}ms)`)
  console.log(`${stepLabel} output:\n${text || '[empty]'}`)
  const toolResults = extractToolResults(response)
  if (toolResults.length > 0) {
    console.log(`${stepLabel} tool calls:\n${formatToolCalls(toolResults)}`)
  }
  return { text }
}

function extractToolResults(response: unknown): unknown[] {
  if (!response || typeof response !== 'object') {
    return []
  }
  const record = response as Record<string, unknown>
  const direct = Array.isArray(record.toolResults) ? record.toolResults : []
  if (direct.length > 0) {
    return direct
  }
  const nested = record.response
  if (nested && typeof nested === 'object') {
    const nestedRecord = nested as Record<string, unknown>
    if (Array.isArray(nestedRecord.toolResults)) {
      return nestedRecord.toolResults
    }
  }
  return []
}

function safeJson(value: unknown): string {
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return '"[unserializable]"'
  }
}

function safeJsonCompact(value: unknown): string {
  try {
    return JSON.stringify(value)
  } catch {
    return '"[unserializable]"'
  }
}

function formatToolCalls(results: unknown[]): string {
  const lines: string[] = []
  for (const entry of results) {
    if (!entry || typeof entry !== 'object') {
      lines.push(safeJsonCompact(entry))
      continue
    }
    const record = entry as Record<string, unknown>
    const payload = (record.payload && typeof record.payload === 'object')
      ? (record.payload as Record<string, unknown>)
      : record
    const toolName = typeof payload.toolName === 'string'
      ? payload.toolName
      : (typeof record.toolName === 'string' ? record.toolName : null)
    const args = (payload as Record<string, unknown>).args
      ?? (record as Record<string, unknown>).args
      ?? null
    const result = (payload as Record<string, unknown>).result
      ?? (record as Record<string, unknown>).result
      ?? null
    if (toolName) {
      lines.push(`${toolName} ${safeJsonCompact(args)} => ${safeJsonCompact(result)}`)
      continue
    }
    lines.push(safeJsonCompact(entry))
  }
  return lines.join('\n')
}

async function ensureMastraDb(dataDir: string): Promise<void> {
  const dbPath = `${dataDir}/mastra.db`
  await access(dbPath)
}

async function main(): Promise<void> {
  const { userId, daysPerChunk, maxChunks } = parseArgs()
  const dataDir = process.env.DATA_DIR ?? './data'
  const openAiModel = process.env.OPENAI_MODEL ?? 'gpt-5-mini'
  const timeZone = process.env.TZ ?? 'Asia/Tokyo'

  await ensureMastraDb(dataDir)

  const mcp = new MCPClient({
    servers: {
      concept_graph: {
        command: './bin/concept-graph',
        args: ['--enable-set-time'],
        env: buildConceptGraphEnv(),
      },
    },
  })

  try {
    const tools = await mcp.client.listTools()
    const toolsets = await mcp.client.listToolsets()
    const agent = summon(dataDir, openAiModel, tools)
    const agentMemory = await agent.getMemory()
    if (!agentMemory) {
      throw new Error('Agent memory is not configured')
    }

    const instructions = await loadInstructions()
    const requestContext = new RequestContext()
    requestContext.set('instructions', instructions)

    const entries = await fetchAllMessages(agentMemory, userId, agent.name)
    if (entries.length === 0) {
      console.log('No messages found')
      return
    }

    const chunks = buildChunks(entries, daysPerChunk, timeZone)
    const totalChunks = chunks.length
    const runChunks = maxChunks ? Math.min(maxChunks, totalChunks) : totalChunks
    console.log(`User: ${userId}`)
    console.log(`Model: ${openAiModel}`)
    console.log(`Data dir: ${dataDir}`)
    console.log(`TZ: ${timeZone}`)
    console.log(`Days per chunk: ${daysPerChunk}`)
    if (maxChunks) {
      console.log(`Max chunks: ${maxChunks}`)
    }
    console.log(`Chunks: ${runChunks}/${totalChunks}`)

    for (let index = 0; index < runChunks; index += 1) {
      const chunk = chunks[index]
      const chunkLogs = chunk.messages
        .map((entry) => `${entry.role}: ${entry.text}`)
        .join('\n')
      const chunkEndMs = chunk.messages[chunk.messages.length - 1]?.createdAt.getTime()
      if (!chunkEndMs) {
        continue
      }

      console.log(`\n[${index + 1}/${runChunks}] ${chunk.startDay} -> ${chunk.endDay} (${chunk.messages.length} messages)`)
      console.log(`Chunk end: ${new Date(chunkEndMs).toISOString()} (${chunkEndMs})`)

      const step1Prompt = `${STEP1_PROMPT}\n\n対象ログ (${chunk.startDay}〜${chunk.endDay}):\n${chunkLogs}`
      const step1Result = await runStep(agent, requestContext, toolsets, step1Prompt, 'Step 1')
      let stepContext = `${step1Prompt}\n\nStep 1 結果:\n${step1Result.text}`

      console.log(`set_time: ${chunkEndMs}`)
      const setTimeResult = await mcp.callTool('concept_graph', 'set_time', { now_ms: chunkEndMs })
      console.log(`set_time result:\n${safeJson(setTimeResult)}`)

      const step2Prompt = `${stepContext}\n\n${STEP2_PROMPT}`
      const step2Result = await runStep(agent, requestContext, toolsets, step2Prompt, 'Step 2')
      stepContext = `${step2Prompt}\n\nStep 2 結果:\n${step2Result.text}`

      const step3Prompt = `${stepContext}\n\n${STEP3_PROMPT}`
      const step3Result = await runStep(agent, requestContext, toolsets, step3Prompt, 'Step 3')
      stepContext = `${step3Prompt}\n\nStep 3 結果:\n${step3Result.text}`

      const step4Prompt = `${stepContext}\n\n${STEP4_PROMPT}`
      const step4Result = await runStep(agent, requestContext, toolsets, step4Prompt, 'Step 4')
      stepContext = `${step4Prompt}\n\nStep 4 結果:\n${step4Result.text}`

      const step5Prompt = `${stepContext}\n\n${STEP5_PROMPT}`
      await runStep(agent, requestContext, toolsets, step5Prompt, 'Step 5')
    }

    console.log('set_time reset')
    const resetResult = await mcp.callTool('concept_graph', 'set_time', { now_ms: 0 })
    console.log(`set_time reset result:\n${safeJson(resetResult)}`)
  } finally {
    mcp[Symbol.dispose]()
  }
}

main().catch((error) => {
  console.error(error)
  process.exit(1)
})
