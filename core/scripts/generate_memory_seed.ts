import fs from 'fs/promises'
import os from 'os'
import path from 'path'
import { execFile as execFileCb } from 'child_process'
import { promisify } from 'util'

import { createOpenAI } from '@ai-sdk/openai'

const execFile = promisify(execFileCb)

type CliOptions = {
  backup?: string
  cacheDir: string
  prompt?: string
  promptFile?: string
  threadId?: string
  resourceId?: string
  limit: number
  model: string
  output?: string
  dryRun: boolean
}

type MessageRow = {
  role: string
  createdAt: string
  content: string
}

type HistoryEntry = {
  role: string
  text: string
  isSystem: boolean
}

function parseArgs(argv: string[]): CliOptions {
  const options: CliOptions = {
    cacheDir: '/tmp/tsuki-mastra-cache',
    limit: 400,
    model: process.env.OPENAI_MODEL || 'gpt-4.1',
    dryRun: false,
  }
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i]
    if (arg === '--') {
      continue
    }
    const value = argv[i + 1]
    switch (arg) {
      case '--backup':
        options.backup = value
        i += 1
        break
      case '--prompt':
        options.prompt = value
        i += 1
        break
      case '--cache-dir':
        options.cacheDir = value
        i += 1
        break
      case '--prompt-file':
        options.promptFile = value
        i += 1
        break
      case '--thread-id':
        options.threadId = value
        i += 1
        break
      case '--resource-id':
        options.resourceId = value
        i += 1
        break
      case '--limit':
        options.limit = Number(value)
        i += 1
        break
      case '--model':
        options.model = value
        i += 1
        break
      case '--output':
        options.output = value
        i += 1
        break
      case '--dry-run':
        options.dryRun = true
        break
      case '--help':
      case '-h':
        printHelpAndExit(0)
        break
      default:
        if (arg.startsWith('--')) {
          throw new Error(`Unknown option: ${arg}`)
        }
    }
  }
  if (!Number.isFinite(options.limit) || options.limit <= 0) {
    throw new Error('--limit must be a positive number')
  }
  if (!options.dryRun && !options.prompt && !options.promptFile) {
    throw new Error('Either --prompt or --prompt-file is required')
  }
  return options
}

function printHelpAndExit(code: number): never {
  console.log(`
Usage:
  pnpm tsx scripts/generate_memory_seed.ts [options]

Options:
  --backup <path>        Backup tar.gz path (default: latest in ../backup)
  --cache-dir <path>     Cache directory for extracted mastra.db (default: /tmp/tsuki-mastra-cache)
  --prompt <text>        Prompt text for memory seed generation
  --prompt-file <path>   Prompt file path (utf-8)
  --thread-id <id>       Optional thread_id filter
  --resource-id <id>     Optional resourceId filter
  --limit <n>            Number of latest messages to use (default: 400)
  --model <name>         OpenAI model name (default: OPENAI_MODEL or gpt-4.1)
  --output <path>        Write generated memory seed to file
  --dry-run              Do not call OpenAI; print history sample only
  --help, -h             Show this help
`)
  process.exit(code)
}

async function getLatestBackupFile(backupDir: string): Promise<string> {
  const files = await fs.readdir(backupDir)
  const tgzFiles = files.filter((f) => f.endsWith('.tar.gz') && f.startsWith('tsuki-backup-'))
  if (tgzFiles.length === 0) {
    throw new Error(`No core backup files found in: ${backupDir}`)
  }
  tgzFiles.sort()
  return path.join(backupDir, tgzFiles[tgzFiles.length - 1])
}

async function listTarEntries(backupFile: string): Promise<string[]> {
  const { stdout } = await execFile('tar', ['-ztf', backupFile], { maxBuffer: 64 * 1024 * 1024 })
  return stdout
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
}

function backupCacheKey(backupFile: string, stat: { mtimeMs: number, size: number }): string {
  const base = path.basename(backupFile).replace(/[^a-zA-Z0-9._-]/g, '_')
  return `${base}_${Math.floor(stat.mtimeMs)}_${stat.size}`
}

async function extractMastraDb(
  backupFile: string,
  cacheRoot: string,
): Promise<{ dbPath: string, fromCache: boolean }> {
  const entries = await listTarEntries(backupFile)
  const dbInArchive = entries
    .filter((entry) => entry.endsWith('mastra.db'))
    .sort()
    .slice(-1)[0]
  if (!dbInArchive) {
    throw new Error(`mastra.db not found in archive: ${backupFile}`)
  }
  const stat = await fs.stat(backupFile)
  const key = backupCacheKey(backupFile, { mtimeMs: stat.mtimeMs, size: stat.size })
  const cacheDir = path.join(cacheRoot, key)
  const dbPath = path.join(cacheDir, dbInArchive)

  try {
    await fs.access(dbPath)
    return { dbPath, fromCache: true }
  } catch {
    // cache miss
  }

  await fs.mkdir(cacheDir, { recursive: true })
  await execFile('tar', ['-xzf', backupFile, '-C', cacheDir, dbInArchive])
  await fs.access(dbPath)
  return { dbPath, fromCache: false }
}

function parsePartText(text: string): { text: string | null, isSystem: boolean } {
  try {
    const parsed = JSON.parse(text) as unknown
    if (typeof parsed === 'string') {
      return { text: parsed.trim() || null, isSystem: false }
    }
    if (parsed && typeof parsed === 'object') {
      const record = parsed as Record<string, unknown>
      const user = record.user
      const isSystem = typeof user === 'string' && user.trim().toLowerCase() === 'system'
      const content = record.content
      if (typeof content === 'string' && content.trim().length > 0) {
        return { text: content.trim(), isSystem }
      }
      if (typeof user === 'string' && user.trim().length > 0) {
        return { text: user.trim(), isSystem }
      }
    }
  } catch {
    // treat as plain text below
  }
  return { text: text.trim() || null, isSystem: false }
}

function isSystemLikeMessage(text: string): boolean {
  const normalized = text.trim()
  if (!normalized) {
    return false
  }
  if (normalized.startsWith('[source:MCP:')) {
    return true
  }
  if (normalized.startsWith('Received scheduler notification:')) {
    return true
  }
  return false
}

function simplifyMessage(row: MessageRow): HistoryEntry[] {
  let root: unknown
  try {
    root = JSON.parse(row.content)
  } catch {
    const plain = row.content.trim()
    if (!plain) {
      return []
    }
    const isSystem = row.role.trim().toLowerCase() === 'system' || isSystemLikeMessage(plain)
    return [{ role: row.role, text: plain, isSystem }]
  }
  if (!root || typeof root !== 'object') {
    return []
  }
  const parts = (root as Record<string, unknown>).parts
  if (!Array.isArray(parts)) {
    return []
  }
  const entries: HistoryEntry[] = []
  const rowRole = row.role.trim().toLowerCase()
  const rowIsSystem = rowRole === 'system'
  for (const part of parts) {
    if (!part || typeof part !== 'object') continue
    const p = part as Record<string, unknown>
    if (p.type !== 'text') continue
    if (typeof p.text !== 'string') continue
    const parsed = parsePartText(p.text)
    if (!parsed.text) continue
    entries.push({
      role: row.role,
      text: parsed.text,
      isSystem: rowIsSystem || parsed.isSystem || isSystemLikeMessage(parsed.text),
    })
  }
  return entries
}

function filterSystemAndSystemResponses(entries: HistoryEntry[]): HistoryEntry[] {
  const filtered: HistoryEntry[] = []
  let skipNextAssistant = false
  for (const entry of entries) {
    const role = entry.role.trim().toLowerCase()
    if (entry.isSystem || role === 'system') {
      skipNextAssistant = true
      continue
    }
    if (skipNextAssistant && role === 'assistant') {
      skipNextAssistant = false
      continue
    }
    skipNextAssistant = false
    filtered.push(entry)
  }
  return filtered
}

async function loadMessageHistory(dbPath: string, options: CliOptions): Promise<string> {
  const conditions: string[] = []

  if (options.threadId) {
    conditions.push(`thread_id = ${sqlString(options.threadId)}`)
  }
  if (options.resourceId) {
    conditions.push(`resourceId = ${sqlString(options.resourceId)}`)
  }

  const where = conditions.length > 0 ? `WHERE ${conditions.join(' AND ')}` : ''
  const sql = `
    SELECT role, createdAt, content
    FROM mastra_messages
    ${where}
    ORDER BY createdAt DESC
    LIMIT ${Math.floor(options.limit)}
  `
  const { stdout } = await execFile('sqlite3', ['-json', dbPath, sql], { maxBuffer: 64 * 1024 * 1024 })
  const rows = JSON.parse(stdout || '[]') as MessageRow[]
  rows.reverse()
  const entries = rows.flatMap((row) => simplifyMessage(row))
  const filteredEntries = filterSystemAndSystemResponses(entries)
  const lines = filteredEntries.map((entry) => `${entry.role}: ${entry.text}`)
  return lines.join('\n')
}

function sqlString(value: string): string {
  return `'${value.replaceAll("'", "''")}'`
}

async function loadPrompt(options: CliOptions): Promise<string> {
  if (options.prompt && options.prompt.trim().length > 0) {
    return options.prompt.trim()
  }
  if (!options.promptFile) {
    throw new Error('Prompt is required')
  }
  const content = await fs.readFile(options.promptFile, 'utf-8')
  if (content.trim().length === 0) {
    throw new Error(`Prompt file is empty: ${options.promptFile}`)
  }
  return content.trim()
}

function extractGeneratedText(result: unknown): string {
  if (!result || typeof result !== 'object') {
    return ''
  }
  const root = result as Record<string, unknown>
  if (typeof root.text === 'string' && root.text.trim().length > 0) {
    return root.text.trim()
  }

  const content = root.content
  if (Array.isArray(content)) {
    const fromContent = content
      .flatMap((item) => {
        if (!item || typeof item !== 'object') return []
        const c = item as Record<string, unknown>
        if (c.type !== 'text') return []
        if (typeof c.text !== 'string') return []
        return [c.text.trim()]
      })
      .filter((text) => text.length > 0)
      .join('\n\n')
    if (fromContent.length > 0) {
      return fromContent
    }
  }

  const response = root.response
  if (response && typeof response === 'object') {
    const body = (response as Record<string, unknown>).body
    if (body && typeof body === 'object') {
      const output = (body as Record<string, unknown>).output
      if (Array.isArray(output)) {
        const fromOutputText = output
          .flatMap((entry) => {
            if (!entry || typeof entry !== 'object') return []
            const contentBlocks = (entry as Record<string, unknown>).content
            if (!Array.isArray(contentBlocks)) return []
            return contentBlocks.flatMap((block) => {
              if (!block || typeof block !== 'object') return []
              const b = block as Record<string, unknown>
              if (b.type !== 'output_text') return []
              if (typeof b.text !== 'string') return []
              return [b.text.trim()]
            })
          })
          .filter((text) => text.length > 0)
          .join('\n\n')
        if (fromOutputText.length > 0) {
          return fromOutputText
        }
      }
    }
  }
  return ''
}

async function generateMemorySeed(history: string, prompt: string, modelName: string): Promise<string> {
  const apiKey = process.env.OPENAI_API_KEY
  if (!apiKey) {
    throw new Error('OPENAI_API_KEY is required when --dry-run is not set')
  }
  const openai = createOpenAI({ apiKey })
  const model = await openai(modelName)
  const result = await model.doGenerate({
    prompt: [
      { role: 'system', content: prompt },
      { role: 'user', content: [{ type: 'text', text: history }] },
    ],
    inputFormat: 'prompt',
    mode: { type: 'regular' },
  })
  return extractGeneratedText(result)
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2))
  const backupPath =
    options.backup || (await getLatestBackupFile(path.resolve(process.cwd(), '../backup')))

  const cacheRoot = path.resolve(options.cacheDir || path.join(os.tmpdir(), 'tsuki-mastra-cache'))
  const { dbPath, fromCache } = await extractMastraDb(backupPath, cacheRoot)
  const history = await loadMessageHistory(dbPath, options)
  if (!history.trim()) {
    throw new Error('No messages found for the given filters')
  }

  console.log(`backup: ${backupPath}`)
  console.log(`cache dir: ${cacheRoot}`)
  console.log(`db: ${dbPath}`)
  console.log(`cache hit: ${fromCache ? 'yes' : 'no'}`)
  console.log(`history chars: ${history.length}`)
  console.log(`model: ${options.model}`)

  if (options.dryRun) {
    console.log('--- history preview ---')
    console.log(history)
    return
  }

  const prompt = await loadPrompt(options)
  const generated = await generateMemorySeed(history, prompt, options.model)
  if (!generated) {
    throw new Error('Model returned empty output')
  }

  console.log('--- generated memory seed ---')
  console.log(generated)

  if (options.output) {
    await fs.mkdir(path.dirname(options.output), { recursive: true })
    await fs.writeFile(options.output, `${generated}\n`, 'utf-8')
    console.log(`written: ${options.output}`)
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
