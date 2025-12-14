/**
 * Router evaluation helper.
 *
 * Usage:
 *   pnpm tsx scripts/test_router.ts prepare [--out path] [--limit N] [--feed-id ID]
 *   pnpm tsx scripts/test_router.ts [--in path] [--limit N] [--model MODEL] [--max-log N] [--instructions-file path]
 *
 * prepare: fetches RSS articles via universal MCP and saves sensory lines to JSONL.
 * default: reads JSONL and runs AIRouter decisions over each sensory line.
 */

import { readFile, writeFile } from 'fs/promises'
import { fileURLToPath } from 'url'
import path from 'path'
import { AIRouter } from '../src/agent/aiRouter'
import { getUniversalMCP } from '../src/mastra/mcp'
import { loadPromptFromEnv } from '../src/agent/prompt'
import { randomBytes } from 'crypto'

type Mode = 'prepare' | 'run'

type Args = Record<string, string | undefined>

type SensoryRecord = { text: string }

type RssOptions = {
  outPath: string
  limit: number
  feedId?: string
}

type RunOptions = {
  inPath: string
  limit?: number
  model: string
  maxLog: number
  instructionsFile?: string
}

const __filename = fileURLToPath(import.meta.url)
const __dirname = path.dirname(__filename)

function parseArgs(argv: string[]): { mode: Mode; args: Args } {
  const [first, ...rest] = argv
  const mode: Mode = first === 'prepare' ? 'prepare' : 'run'
  const tail = mode === 'prepare' ? rest : argv
  const args: Args = {}
  for (let i = 0; i < tail.length; i++) {
    const token = tail[i]
    if (token?.startsWith('--')) {
      const key = token.slice(2)
      const value = tail[i + 1] && !tail[i + 1].startsWith('--') ? tail[++i] : 'true'
      args[key] = value
    }
  }
  return { mode, args }
}

function numberArg(args: Args, key: string, fallback: number): number {
  const raw = args[key]
  const num = raw ? Number(raw) : NaN
  if (Number.isNaN(num) || num <= 0) {
    return fallback
  }
  return num
}

async function readJsonl(filePath: string): Promise<SensoryRecord[]> {
  const content = await readFile(filePath, 'utf8')
  return content
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean)
    .map((line) => JSON.parse(line) as SensoryRecord)
    .filter((rec) => typeof rec.text === 'string' && rec.text.trim().length > 0)
}

async function writeJsonl(filePath: string, records: SensoryRecord[]): Promise<void> {
  const lines = records.map((rec) => JSON.stringify(rec))
  await writeFile(filePath, `${lines.join('\n')}\n`, 'utf8')
}

function extractTextFromResult(result: unknown): string | null {
  if (typeof result === 'string') {
    return result
  }
  if (result && typeof result === 'object') {
    const content = (result as { content?: unknown }).content
    if (Array.isArray(content)) {
      for (const part of content) {
        if (
          part &&
          typeof part === 'object' &&
          'text' in part &&
          typeof (part as { text: unknown }).text === 'string'
        ) {
          return (part as { text: string }).text
        }
      }
    }
  }
  return null
}

function toonLinesToSensory(text: string, limit: number, sourceLabel: string): SensoryRecord[] {
  const lines = text
    .split('\n')
    .map((l) => l.trim())
    .filter((l) => l.length > 0 && !l.startsWith('articles['))

  const seen = new Set<string>()
  const records: SensoryRecord[] = []

  for (const line of lines) {
    if (records.length >= limit) {
      break
    }
    if (seen.has(line)) {
      continue
    }
    seen.add(line)

    records.push({
      text: `[source:${sourceLabel}] ${line}`,
    })
  }

  return records
}

async function prepareRss(options: RssOptions): Promise<void> {
  using mcp = getUniversalMCP()

  const params: Record<string, unknown> = { n: options.limit }
  if (options.feedId) {
    params.feed_id = options.feedId
  }

  const result = await mcp.callTool('rss', 'get_articles', params)
  const text = extractTextFromResult(result)
  if (!text) {
    throw new Error('No text content returned from rss get_articles')
  }

  const records = toonLinesToSensory(text, options.limit, `RSS${options.feedId ? ` ${options.feedId}` : ''}`)
  await writeJsonl(options.outPath, records)
  console.log(`prepare: wrote ${records.length} sensory lines to ${options.outPath}`)
}

async function loadInstructions(filePath?: string): Promise<string> {
  if (filePath) {
    return readFile(filePath, 'utf8')
  }

  try {
    return await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
  } catch (err) {
    console.warn('Failed to load encrypted prompt; falling back to minimal router instructions', err)
    return 'You are a routing filter. Output respond, ignore, or maybe.'
  }
}

function snippet(text: string, max = 120): string {
  if (text.length <= max) return text
  return `${text.slice(0, max - 3)}...`
}

async function runRouter(options: RunOptions): Promise<void> {
  const records = await readJsonl(options.inPath)
  const limited = options.limit ? records.slice(0, options.limit) : records

  const instructions = await loadInstructions(options.instructionsFile)
  const router = new AIRouter(options.model, instructions, options.maxLog)

  let idx = 0
  for (const rec of limited) {
    idx += 1
    try {
      const decision = await router.route({
        userId: 'router-test',
        type: 'sensory',
        text: rec.text,
      })
      console.log(
        `${String(idx).padStart(3, '0')} | ${decision.action.padEnd(7)} | ${snippet(rec.text)}`,
      )
    } catch (err) {
      console.error(`Error routing line ${idx}:`, err)
    }
  }
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2)
  const { mode, args } = parseArgs(argv)

  if (mode === 'prepare') {
    const outPath = path.resolve(__dirname, args.out ?? 'test_router.samples.jsonl')
    const limit = numberArg(args, 'limit', 20)
    const feedId = args['feed-id']
    await prepareRss({ outPath, limit, feedId })
    return
  }

  // run mode
  const inPath = path.resolve(__dirname, args.in ?? 'test_router.samples.jsonl')
  const limit = args.limit ? numberArg(args, 'limit', 0) : undefined
  const model = args.model ?? process.env.ROUTER_MODEL ?? 'gpt-4o-mini'
  const maxLog = numberArg(args, 'max-log', 20)
  const instructionsFile = args['instructions-file']

  // Seed not strictly needed, but touch crypto to avoid tree-shaking warnings in some bundlers.
  randomBytes(4)

  await runRouter({ inPath, limit, model, maxLog, instructionsFile })
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
