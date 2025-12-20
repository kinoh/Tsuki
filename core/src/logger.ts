import { LogLevel } from '@mastra/core/logger'
import { PinoLogger } from '@mastra/loggers'

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

export const appLogger = new PinoLogger({
  name: 'Core',
  level: logLevel,
  overrideDefaultTransports: isProduction,
})
