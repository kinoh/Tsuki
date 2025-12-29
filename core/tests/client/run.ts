import { WebSocket } from 'ws'
import fs from 'fs/promises'
import path from 'path'
import pino from 'pino'
import { parse } from 'yaml'

type InputItem = {
  text: string
  type?: 'message' | 'sensory'
}

type Scenario = {
  inputs: InputItem[]
}

const WS_URL = process.env.WS_URL ?? 'ws://localhost:2953/'
const AUTH_TOKEN = process.env.WEB_AUTH_TOKEN ?? 'test-token'
const USER_NAME = process.env.USER_NAME ?? 'test-user'
const LOG_DIR = process.env.LOG_DIR ?? path.resolve(process.cwd(), 'tests/client/logs')
const RESPONSE_TIMEOUT_MS = 10000

type ServerMessage = {
  role: 'user' | 'assistant' | 'system' | 'tool'
  user: string
  chat: string[]
  timestamp: number
}

function formatTimestamp(date: Date): string {
  const pad = (value: number): string => value.toString().padStart(2, '0')
  const yyyy = date.getFullYear()
  const mm = pad(date.getMonth() + 1)
  const dd = pad(date.getDate())
  const hh = pad(date.getHours())
  const mi = pad(date.getMinutes())
  const ss = pad(date.getSeconds())
  return `${yyyy}${mm}${dd}-${hh}${mi}${ss}`
}

function parseScenario(text: string): Scenario {
  const data = parse(text)
  if (!data || typeof data !== 'object') {
    throw new Error('Scenario must be a YAML object')
  }

  const inputs = (data as { inputs?: unknown }).inputs
  if (!Array.isArray(inputs)) {
    throw new Error('Scenario must have inputs: []')
  }

  const normalized: InputItem[] = inputs.map((item, index) => {
    if (!item || typeof item !== 'object') {
      throw new Error(`inputs[${index}] must be an object`)
    }
    const { text, type } = item as { text?: unknown; type?: unknown }
    if (typeof text !== 'string' || text.trim().length === 0) {
      throw new Error(`inputs[${index}].text must be a non-empty string`)
    }
    if (type !== undefined && type !== 'message' && type !== 'sensory') {
      throw new Error(`inputs[${index}].type must be "message" or "sensory"`)
    }
    return { text, type: (type as 'message' | 'sensory' | undefined) ?? 'message' }
  })

  return { inputs: normalized }
}

function isServerMessage(value: unknown): value is ServerMessage {
  if (!value || typeof value !== 'object') return false
  const maybe = value as Record<string, unknown>
  if (maybe.role !== 'user' && maybe.role !== 'assistant' && maybe.role !== 'system' && maybe.role !== 'tool') {
    return false
  }
  if (typeof maybe.user !== 'string') return false
  if (!Array.isArray(maybe.chat) || maybe.chat.some((entry) => typeof entry !== 'string')) return false
  if (typeof maybe.timestamp !== 'number') return false
  return true
}

async function ensureLogFile(): Promise<string> {
  await fs.mkdir(LOG_DIR, { recursive: true })
  const fileName = `${formatTimestamp(new Date())}.jsonl`
  return path.join(LOG_DIR, fileName)
}

async function loadScenario(scenarioPath: string): Promise<Scenario> {
  const content = await fs.readFile(scenarioPath, 'utf-8')
  return parseScenario(content)
}

async function run(): Promise<void> {
  const scenarioPath = process.argv[2]
  if (!scenarioPath) {
    console.error('Usage: pnpm tsx ./tests/client/run.ts <scenario.yaml>')
    process.exit(1)
  }

  const scenario = await loadScenario(scenarioPath)
  const logPath = await ensureLogFile()
  const fileDestination = pino.destination({ dest: logPath, sync: false })
  const logger = pino(
    { level: 'info' },
    pino.multistream([
      { stream: process.stdout },
      { stream: fileDestination },
    ]),
  )

  logger.info({ event: 'start', scenarioPath })
  logger.info({ event: 'connect', url: WS_URL })

  const ws = new WebSocket(WS_URL)

  let closed = false
  let pendingResponse: {
    resolve: () => void
    reject: (error: Error) => void
    timer: NodeJS.Timeout
  } | null = null

  const closeOnce = (code?: number, reason?: string): void => {
    if (closed) return
    closed = true
    logger.info({ event: 'close', code, reason })
    ws.close()
  }

  const clearPending = (): void => {
    if (!pendingResponse) return
    clearTimeout(pendingResponse.timer)
    pendingResponse = null
  }

  const rejectPending = (error: Error): void => {
    if (!pendingResponse) return
    const { reject } = pendingResponse
    clearPending()
    reject(error)
  }

  const waitForServerMessage = (timeoutMs: number): Promise<void> => {
    if (pendingResponse) {
      return Promise.reject(new Error('Pending response already exists'))
    }
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pendingResponse = null
        reject(new Error(`Timed out waiting for server response after ${timeoutMs}ms`))
      }, timeoutMs)
      pendingResponse = { resolve: () => {
        clearTimeout(timer)
        pendingResponse = null
        resolve()
      }, reject, timer }
    })
  }

  ws.on('open', () => {
    logger.info({ event: 'open' })
    const auth = `${USER_NAME}:${AUTH_TOKEN}`
    ws.send(auth)
    logger.info({ event: 'auth_sent', user: USER_NAME })

    void (async () => {
      for (const input of scenario.inputs) {
        const payload = { type: input.type ?? 'message', text: input.text }
        ws.send(JSON.stringify(payload))
        logger.info({ event: 'send', payload })
        await waitForServerMessage(RESPONSE_TIMEOUT_MS)
      }
      closeOnce()
    })().catch((error) => {
      logger.error({ event: 'scenario_error', message: error.message })
      process.exitCode = 1
      closeOnce()
    })
  })

  ws.on('message', (data) => {
    const raw = data.toString()
    try {
      const parsed = JSON.parse(raw) as unknown
      logger.info({ event: 'receive', message: parsed })
      if (pendingResponse && isServerMessage(parsed)) {
        if (parsed.chat.includes('Internal error!')) {
          logger.error({ event: 'server_error', message: 'Internal error!' })
          process.exitCode = 1
          rejectPending(new Error('Internal error from server'))
          closeOnce()
          return
        }
        pendingResponse.resolve()
      }
    } catch {
      logger.info({ event: 'receive', message: raw })
    }
  })

  ws.on('error', (error) => {
    logger.error({ event: 'error', message: error.message })
    rejectPending(new Error(error.message))
    closeOnce()
  })

  ws.on('close', (code, reason) => {
    logger.info({ event: 'closed', code, reason: reason.toString() })
    rejectPending(new Error('WebSocket closed'))
    fileDestination.end()
  })
}

run().catch((error) => {
  console.error(error)
  process.exit(1)
})
