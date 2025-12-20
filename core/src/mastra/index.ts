
import { Agent, ToolsInput } from '@mastra/core/agent'
import { Mastra } from '@mastra/core/mastra'
import { LibSQLStore } from '@mastra/libsql'
import { summon } from './agents/tsuki'
import { getUniversalMCP, MCPClient } from './mcp'
import { Metric, ToolAction } from '@mastra/core'
import { ConsoleLogger } from '@mastra/core/logger'
import { ConfigService } from '../configService'

export class MastraInstance {
  constructor(
    public readonly mastra: Mastra<Record<string, Agent<string, ToolsInput, Record<string, Metric>>>>,
    public readonly mcp: MCPClient,
  ) {
  }

  public static async create(config: ConfigService): Promise<MastraInstance> {
    const dataDir = config.dataDir

    const openAiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'

    const mcp = getUniversalMCP(config)
    const tools = await mcp.client.getTools()

    const mastra = new Mastra({
      workflows: {},
      agents: { tsuki: summon(dataDir, openAiModel, tools) },
      storage: new LibSQLStore({
        url: `file:${dataDir}/mastra.db`,
      }),
      logger: new ConsoleLogger({
        name: 'Mastra',
        level: 'info',
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
