import fs from 'fs/promises'
import path from 'path'
import { exec as execCb } from 'child_process'
import { promisify } from 'util'
import { createClient } from '@libsql/client'
import { createOpenAI, OpenAIProvider } from '@ai-sdk/openai'
import { loadPromptFromEnv } from '../src/agent/prompt'

const exec = promisify(execCb)

async function getLatestBackupFile(backupDir: string): Promise<string> {
  const files = await fs.readdir(backupDir)
  const tgzFiles = files.filter(f => f.endsWith('.tar.gz'))
  if (tgzFiles.length === 0) throw new Error('No backup files found')
  tgzFiles.sort()
  return path.join(backupDir, tgzFiles[tgzFiles.length - 1])
}

async function extractMastraDb(backupFile: string, outDir: string): Promise<string> {
  await fs.mkdir(outDir, { recursive: true })
  // List archive contents and find all paths ending with mastra.db
  const { stdout: tarList } = await exec(`tar -ztf ${backupFile}`)
  const dbCandidates = tarList.split('\n').filter(line => line.trim().endsWith('mastra.db'))
  if (dbCandidates.length === 0) throw new Error('mastra.db not found in archive')
  // Pick the last (most recent by name sort)
  const dbInArchive = dbCandidates.sort().slice(-1)[0]
  // Extract only that file
  await exec(`tar -xzf ${backupFile} -C ${outDir} '${dbInArchive}'`)
  const dbPath = path.join(outDir, dbInArchive)
  // Ensure the file exists
  try {
    await fs.access(dbPath)
  } catch {
    throw new Error(`Failed to extract mastra.db: ${dbPath}`)
  }
  return dbPath
}

function simplifyContents(rows: string[]): string[] {
  return rows.flatMap(row => {
    try {
      const obj = JSON.parse(row)
      if (typeof obj === 'object' && obj !== null && 'parts' in obj) {
        return (obj.parts as { text: string }[]).map(part => {
          if ('text' in part && typeof part.text === 'string') {
            const message = JSON.parse(part.text)
            const user = message.user || 'tsuki'
            return `${user}: ${message.content}`
          } else {
            return ''
          }
        })
      }
    } catch (e) {
      console.error('Failed to parse row as JSON:', e)
    }
    console.warn('Unrecognized message format, skipping:', row)
    return []
  }).filter(line => line.trim().length > 0)
}

async function getAllContents(dbPath: string): Promise<string[]> {
  const db = createClient({ url: `file:${dbPath}` })
  const result = await db.execute('SELECT content FROM mastra_messages')
  return result.rows.map(r => r.content as string)
}

async function summarize(content: string, openai: OpenAIProvider): Promise<string> {
  const initialPrompt = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
  const summarizePrompt = `以下はユーザーとの過去の会話です。関係性を深めるために重要なポイントを日本語でmarkdownの箇条書きで要約してください。例: \n- 🎹王道の次を探してた→メトネルやスクリャービンを紹介\n- ✨「つき先生」って呼ばれて照れた`

  const model = await openai('gpt-4.1')
  const result = await model.doGenerate({
    prompt: [
      {
        role: 'system', content: `${initialPrompt}\n\n${summarizePrompt}`,
      },
      {
        role: 'user', content: [{ type: 'text', text: content }],
      }
    ],
    inputFormat: 'prompt',
    mode: {
      type: 'regular',
    }
  })

  console.log('Usage:', result.usage)
  console.log('Provider metadata:', result.providerMetadata)

  return result.text ?? 'No summary generated'
}

async function main() {
  const backupDir = '../backup'
  const tmpDir = '/tmp/mastra_db_extract'
  const openaiApiKey = process.env.OPENAI_API_KEY
  if (!openaiApiKey) throw new Error('OPENAI_API_KEY is not set')

  const latestBackup = await getLatestBackupFile(backupDir)
  const dbPath = await extractMastraDb(latestBackup, tmpDir)
  const contents = simplifyContents(await getAllContents(dbPath))

  console.log(contents.join('\n'))
  console.log('---')

  const openai = createOpenAI({ apiKey: openaiApiKey })
  const summary = await summarize(contents.join('\n'), openai)

  console.log(summary)
  console.log('---')
}

main().catch(e => { console.error(e); process.exit(1); })
