import { LogLevel } from '@mastra/core/logger'
import pino from 'pino'
import pretty from 'pino-pretty'

export const parseLogLevel = (value?: string): LogLevel => {
  switch (value?.toLowerCase()) {
    case LogLevel.DEBUG:
      return LogLevel.DEBUG
    case LogLevel.INFO:
      return LogLevel.INFO
    case LogLevel.WARN:
      return LogLevel.WARN
    case LogLevel.ERROR:
      return LogLevel.ERROR
    case LogLevel.NONE:
      return LogLevel.NONE
    default:
      return LogLevel.INFO
  }
}

const env = process.env.ENV ?? process.env.NODE_ENV ?? 'development'
const isProduction = env === 'production'
const logLevel = parseLogLevel(process.env.LOG_LEVEL)

type LogArgs = Record<string, unknown>

const normalizeArgs = (args: LogArgs): LogArgs => {
  if (!args || typeof args !== 'object') {
    return {}
  }

  if ('err' in args) {
    return args
  }

  if ('error' in args) {
    const { error, ...rest } = args as { error?: unknown }
    if (error !== undefined) {
      return { ...rest, err: error }
    }
    return rest
  }

  return args
}

const prettyStream = isProduction
  ? undefined
  : pretty({
      colorize: true,
      levelFirst: true,
      ignore: 'pid,hostname',
      colorizeObjects: true,
      translateTime: 'SYS:standard',
      singleLine: false,
    })

const baseLogger = pino(
  {
    name: 'Core',
    level: logLevel,
  },
  prettyStream,
)

class AppLogger {
  constructor(private logger: pino.Logger) {}

  child(bindings: LogArgs): AppLogger {
    return new AppLogger(this.logger.child(bindings))
  }

  debug(message: string, args: LogArgs = {}): void {
    this.logger.debug(normalizeArgs(args), message)
  }

  info(message: string, args: LogArgs = {}): void {
    this.logger.info(normalizeArgs(args), message)
  }

  warn(message: string, args: LogArgs = {}): void {
    this.logger.warn(normalizeArgs(args), message)
  }

  error(message: string, args: LogArgs = {}): void {
    this.logger.error(normalizeArgs(args), message)
  }
}

export const appLogger = new AppLogger(baseLogger)
