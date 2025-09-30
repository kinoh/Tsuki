
import { Agent, ToolsInput } from '@mastra/core/agent'
import { Mastra } from '@mastra/core/mastra'
import { PinoLogger } from '@mastra/loggers'
import { LibSQLStore } from '@mastra/libsql'
import { mkdirSync } from 'fs'
import { summon } from './agents/tsuki'
import { getUniversalMCP, MCPClient } from './mcp'
import { Metric, ToolAction } from '@mastra/core'

export class MastraInstance {
  constructor(
    public readonly mastra: Mastra<Record<string, Agent<string, ToolsInput, Record<string, Metric>>>>,
    private readonly mcp: MCPClient,
  ) {
  }

  public static async create(): Promise<MastraInstance> {
    // Initialize data directory
    const dataDir = process.env.DATA_DIR ?? './data'
    mkdirSync(dataDir, { recursive: true })

    const openAiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'

    const mcp = getUniversalMCP()
    const tools = await mcp.client.getTools()

    const mastra = new Mastra({
      workflows: {},
      agents: { tsuki: summon(dataDir, openAiModel, tools) },
      storage: new LibSQLStore({
        url: `file:${dataDir}/mastra.db`,
      }),
      logger: new PinoLogger({
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
