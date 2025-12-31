import { spawn } from 'node:child_process'
import net from 'node:net'
import { URL } from 'node:url'

const WS_URL = process.env.WS_URL ?? 'ws://localhost:2953/'
const TIMEOUT_MS = 60000
const INTERVAL_MS = 500
const USAGE = 'Usage: pnpm tsx ./tests/runner.ts <script> [args...]'

type ChildProc = ReturnType<typeof spawn>
type ExitResult = { code: number | null; signal: NodeJS.Signals | null }
type StopResult = { exit: ExitResult; sentSignal: NodeJS.Signals | null }

const sleep = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms))

const waitForExit = (proc: ChildProc, timeoutMs?: number): Promise<ExitResult> => new Promise((resolve) => {
  if (proc.exitCode !== null || proc.signalCode !== null) {
    resolve({ code: proc.exitCode, signal: proc.signalCode })
    return
  }
  const onExit = (code: number | null, signal: NodeJS.Signals | null): void => {
    if (timer) clearTimeout(timer)
    resolve({ code, signal })
  }
  const timer = timeoutMs
    ? setTimeout(() => {
      proc.removeListener('exit', onExit)
      resolve({ code: proc.exitCode, signal: proc.signalCode })
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

const stop = async (proc: ChildProc | null): Promise<StopResult | null> => {
  if (!proc) return null
  if (proc.exitCode !== null || proc.signalCode !== null) {
    return { exit: { code: proc.exitCode, signal: proc.signalCode }, sentSignal: null }
  }
  const pid = proc.pid
  let sentSignal: NodeJS.Signals | null = null
  if (pid && !killGroup(pid, 'SIGINT')) {
    proc.kill('SIGINT')
  }
  sentSignal = 'SIGINT'
  let exit = await waitForExit(proc, 5000)
  if (exit.code !== null || exit.signal !== null) {
    return { exit, sentSignal }
  }
  if (pid && !killGroup(pid, 'SIGTERM')) {
    proc.kill('SIGTERM')
  }
  sentSignal = 'SIGTERM'
  exit = await waitForExit(proc, 5000)
  return { exit, sentSignal }
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
    const exitResult = await waitForExit(scriptProc)
    process.exitCode = exitResult.code ?? 1
  } finally {
    await stop(scriptProc)
    const coreStop = await stop(coreProc)
    if (coreStop && coreStop.exit.code !== null && coreStop.exit.code !== 0) {
      // Ignore SIGINT sent by us
      const ignoreSignal = coreStop.sentSignal === 'SIGINT' && coreStop.exit.signal === 'SIGINT'
      if (!ignoreSignal) {
        process.exitCode = process.exitCode && process.exitCode !== 0 ? process.exitCode : 1
      }
    }
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.message : String(error))
  console.error(USAGE)
  process.exit(1)
})
