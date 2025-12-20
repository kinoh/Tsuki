
import { Agent, ToolsInput } from '@mastra/core/agent'
import { Mastra } from '@mastra/core/mastra'
import { LibSQLStore } from '@mastra/libsql'
import { summon } from './agents/tsuki'
import { getUniversalMCP, MCPClient } from './mcp'
import { Metric, ToolAction } from '@mastra/core'
import { LogLevel } from '@mastra/core/logger'
import { PinoLogger } from '@mastra/loggers'
import { ConfigService } from '../configService'

const parseLogLevel = (value?: string): LogLevel => {
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

export class MastraInstance {
  constructor(
    public readonly mastra: Mastra<Record<string, Agent<string, ToolsInput, Record<string, Metric>>>>,
    public readonly mcp: MCPClient,
  ) {
  }

  public static async create(config: ConfigService): Promise<MastraInstance> {
    const dataDir = config.dataDir

    const openAiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'
    const logLevel = parseLogLevel(process.env.LOG_LEVEL)

    const mcp = getUniversalMCP(config)
    const tools = await mcp.client.getTools()

    const mastra = new Mastra({
      workflows: {},
      agents: { tsuki: summon(dataDir, openAiModel, tools) },
      storage: new LibSQLStore({
        url: `file:${dataDir}/mastra.db`,
      }),
      logger: new PinoLogger({
        name: 'Mastra',
        level: logLevel,
        overrideDefaultTransports: config.isProduction,
      }),
    })

    return new MastraInstance(mastra, mcp)
  }

  [Symbol.dispose](): void {
    this.mcp[Symbol.dispose]()
  }

  
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  public getAgent(name: string): Agent<string, Record<string, ToolAction<any, any, any>>, Record<string, Metric>> {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    return this.mastra.getAgent(name) as Agent<string, Record<string, ToolAction<any, any, any>>, Record<string, Metric>>
  }
}
