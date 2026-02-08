import fs from 'fs/promises'
import os from 'os'
import path from 'path'
import { execFile as execFileCb } from 'child_process'
import { promisify } from 'util'

import { createOpenAI } from '@ai-sdk/openai'

const execFile = promisify(execFileCb)

type CliOptions = {
  backup?: string
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

function parseArgs(argv: string[]): CliOptions {
  const options: CliOptions = {
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
  if (!options.prompt && !options.promptFile) {
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

async function extractMastraDb(backupFile: string, outDir: string): Promise<string> {
  const entries = await listTarEntries(backupFile)
  const dbInArchive = entries
    .filter((entry) => entry.endsWith('mastra.db'))
    .sort()
    .slice(-1)[0]
  if (!dbInArchive) {
    throw new Error(`mastra.db not found in archive: ${backupFile}`)
  }
  await fs.mkdir(outDir, { recursive: true })
  await execFile('tar', ['-xzf', backupFile, '-C', outDir, dbInArchive])
  const dbPath = path.join(outDir, dbInArchive)
  await fs.access(dbPath)
  return dbPath
}

function parsePartText(text: string): string | null {
  try {
    const parsed = JSON.parse(text) as unknown
    if (typeof parsed === 'string') {
      return parsed.trim() || null
    }
    if (parsed && typeof parsed === 'object') {
      const record = parsed as Record<string, unknown>
      const content = record.content
      if (typeof content === 'string' && content.trim().length > 0) {
        return content.trim()
      }
      const user = record.user
      if (typeof user === 'string' && user.trim().length > 0) {
        return user.trim()
      }
    }
  } catch {
    // treat as plain text below
  }
  return text.trim() || null
}

function simplifyMessage(row: MessageRow): string[] {
  let root: unknown
  try {
    root = JSON.parse(row.content)
  } catch {
    const plain = row.content.trim()
    return plain ? [`${row.role}: ${plain}`] : []
  }
  if (!root || typeof root !== 'object') {
    return []
  }
  const parts = (root as Record<string, unknown>).parts
  if (!Array.isArray(parts)) {
    return []
  }
  const lines: string[] = []
  for (const part of parts) {
    if (!part || typeof part !== 'object') continue
    const p = part as Record<string, unknown>
    if (p.type !== 'text') continue
    if (typeof p.text !== 'string') continue
    const text = parsePartText(p.text)
    if (!text) continue
    lines.push(`${row.role}: ${text}`)
  }
  return lines
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
  const lines = rows.flatMap((row) => simplifyMessage(row))
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
  return result.text?.trim() || ''
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv.slice(2))
  const backupPath =
    options.backup || (await getLatestBackupFile(path.resolve(process.cwd(), '../backup')))

  const tempRoot = await fs.mkdtemp(path.join(os.tmpdir(), 'tsuki-mastra-'))
  try {
    const dbPath = await extractMastraDb(backupPath, tempRoot)
    const history = await loadMessageHistory(dbPath, options)
    if (!history.trim()) {
      throw new Error('No messages found for the given filters')
    }

    console.log(`backup: ${backupPath}`)
    console.log(`db: ${dbPath}`)
    console.log(`history chars: ${history.length}`)

    if (options.dryRun) {
      console.log('--- history preview ---')
      console.log(history.slice(0, 4000))
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
  } finally {
    await fs.rm(tempRoot, { recursive: true, force: true })
  }
}

main().catch((err) => {
  console.error(err)
  process.exit(1)
})
