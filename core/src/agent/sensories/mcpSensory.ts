import { MCPClient } from '../../mastra/mcp'
import { SensoryFetcher, SensorySample } from '../sensoryService'
import { logger } from '../../logger'

export class McpSensory implements SensoryFetcher {
  constructor(
    private readonly mcpClient: MCPClient,
    private readonly serverName: string,
    private readonly toolName: string,
    private readonly params: Record<string, unknown>,
  ) {}

  identifier(): string {
    return `MCP:${this.serverName}/${this.toolName}`
  }

  async fetch(): Promise<SensorySample> {
    let result: unknown

    try {
      result = await this.mcpClient.callTool(
        this.serverName,
        this.toolName,
        this.params,
      )
    } catch (err) {
      logger.error({ err }, 'McpSensory: error fetching notifications')
    }
    return {
      source: this.identifier(),
      text: JSON.stringify(result),
      timestamp: new Date(),
    }
  }
}
