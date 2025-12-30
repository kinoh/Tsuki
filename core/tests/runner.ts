import { spawn } from 'node:child_process'
import net from 'node:net'
import { URL } from 'node:url'

const WS_URL = process.env.WS_URL ?? 'ws://localhost:2953/'
const TIMEOUT_MS = 60000
const INTERVAL_MS = 500
const USAGE = 'Usage: pnpm tsx ./tests/runner.ts <script> [args...]'

type ChildProc = ReturnType<typeof spawn>

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

const waitForExit = (proc: ChildProc, timeoutMs?: number): Promise<number | null> => new Promise((resolve) => {
  if (proc.exitCode !== null) {
    resolve(proc.exitCode)
    return
  }
  const onExit = (code: number | null): void => {
    if (timer) clearTimeout(timer)
    resolve(code)
  }
  const timer = timeoutMs
    ? setTimeout(() => {
      proc.removeListener('exit', onExit)
      resolve(proc.exitCode)
    }, timeoutMs)
    : null
  proc.once('exit', onExit)
})

const killGroup = (pid: number, signal: NodeJS.Signals): boolean => {
  try {
    process.kill(-pid, signal)
    return true
  } catch {
    return false
  }
}

const stop = async (proc: ChildProc | null): Promise<void> => {
  if (!proc || proc.exitCode !== null) return
  const pid = proc.pid
  if (pid && !killGroup(pid, 'SIGINT')) {
    proc.kill('SIGINT')
  }
  await waitForExit(proc, 5000)
  if (proc.exitCode !== null) return
  if (pid && !killGroup(pid, 'SIGTERM')) {
    proc.kill('SIGTERM')
  }
  await waitForExit(proc, 5000)
}

const waitForWs = async (isCoreAlive: () => boolean): Promise<void> => {
  const url = new URL(WS_URL)
  const host = url.hostname
  const port = Number(url.port || (url.protocol === 'wss:' ? 443 : 80))

  const tryConnect = (): Promise<boolean> => new Promise((resolve) => {
    const socket = net.connect({ host, port })
    socket.setTimeout(1000)
    socket.once('connect', () => {
      socket.end()
      resolve(true)
    })
    socket.once('error', () => resolve(false))
    socket.once('timeout', () => {
      socket.destroy()
      resolve(false)
    })
  })

  const start = Date.now()
  while (Date.now() - start < TIMEOUT_MS) {
    if (!isCoreAlive()) {
      throw new Error('core process exited before WS was ready')
    }
    if (await tryConnect()) return
    await sleep(INTERVAL_MS)
  }

  throw new Error(`Timed out waiting for WS: ${WS_URL}`)
}

const parseArgs = (argv: string[]): { script: string; args: string[] } => {
  const [script, ...args] = argv
  if (!script) throw new Error('Script path is required')
  return { script, args }
}

const main = async (): Promise<void> => {
  const { script, args } = parseArgs(process.argv.slice(2))

  let coreProc: ChildProc | null = null
  let scriptProc: ChildProc | null = null

  const forward = (signal: NodeJS.Signals): void => {
    scriptProc?.kill(signal)
    coreProc?.kill(signal)
  }

  process.on('SIGINT', () => forward('SIGINT'))
  process.on('SIGTERM', () => forward('SIGTERM'))

  try {
    coreProc = spawn('pnpm', ['start'], { stdio: 'inherit', env: process.env, detached: true })
    await waitForWs(() => coreProc?.exitCode === null)

    scriptProc = spawn('pnpm', ['tsx', script, ...args], { stdio: 'inherit', env: process.env })
    const exitCode = await waitForExit(scriptProc)
    process.exitCode = exitCode ?? 1
  } finally {
    await stop(scriptProc)
    await stop(coreProc)
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error))
  console.error(USAGE)
  process.exit(1)
})
