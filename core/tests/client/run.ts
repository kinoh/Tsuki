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
const POST_SEND_WAIT_MS = 5000

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
  const destination = pino.destination({ dest: logPath, sync: false })
  const logger = pino({ level: 'info' }, destination)

  logger.info({ event: 'start', scenarioPath })
  logger.info({ event: 'connect', url: WS_URL })

  const ws = new WebSocket(WS_URL)

  let closed = false
  const closeOnce = (code?: number, reason?: string): void => {
    if (closed) return
    closed = true
    logger.info({ event: 'close', code, reason })
    ws.close()
  }

  const scheduleExit = (): void => {
    logger.info({ event: 'post_send_wait', ms: POST_SEND_WAIT_MS })
    setTimeout(() => closeOnce(), POST_SEND_WAIT_MS)
  }

  ws.on('open', () => {
    logger.info({ event: 'open' })
    const auth = `${USER_NAME}:${AUTH_TOKEN}`
    ws.send(auth)
    logger.info({ event: 'auth_sent', user: USER_NAME })

    for (const input of scenario.inputs) {
      const payload = { type: input.type ?? 'message', text: input.text }
      ws.send(JSON.stringify(payload))
      logger.info({ event: 'send', payload })
    }

    scheduleExit()
  })

  ws.on('message', (data) => {
    const raw = data.toString()
    try {
      const parsed = JSON.parse(raw)
      logger.info({ event: 'receive', message: parsed })
    } catch {
      logger.info({ event: 'receive', message: raw })
    }
  })

  ws.on('error', (error) => {
    logger.error({ event: 'error', message: error.message })
    closeOnce()
  })

  ws.on('close', (code, reason) => {
    logger.info({ event: 'closed', code, reason: reason.toString() })
    destination.end()
  })
}

run().catch((error) => {
  console.error(error)
  process.exit(1)
})
