import type { Tool } from '@mastra/core/tools'
import { RequestContext } from '@mastra/core/request-context'
import { MCPClient as MastraMCPClient, MCPClientOptions } from '@mastra/mcp'
import { ConfigService } from '../internal/configService'
import { logger } from '../internal/logger'

export class MCPClient {
  public readonly client: MastraMCPClient

  constructor(options: MCPClientOptions) {
    this.client = new MastraMCPClient(options)
  }

  [Symbol.dispose](): void {
    logger.info('Closing MCP connection...')

    this.client.disconnect().catch((err: unknown) => {
      logger.error({ err }, 'Error disconnecting MCP client')
    })
  }

  public async callTool(serverName: string, toolName: string, params: Record<string, unknown>): Promise<unknown> {
    const toolsets = await this.client.listToolsets()
    const tools = toolsets[serverName]
    if (tools === undefined) {
      throw new Error(`Tool ${toolName} is not available`)
    }
    const tool = tools[toolName] as Tool
    if (tool === undefined) {
      throw new Error(`Tool ${toolName} is not available`)
    }

    if (typeof tool.execute !== 'function') {
      throw new Error(`Tool ${toolName} does not have an executable function`)
    }
    return await tool.execute(params, {
      requestContext: new RequestContext(),
    })
  }
}

export function getUniversalMCP(config: ConfigService): MCPClient {
  return new MCPClient({
    servers: {
      concept_graph: {
        command: './bin/concept-graph',
        args: [],
        env: {
          MEMGRAPH_URI: config.memgraphUri,
          TZ: process.env.TZ ?? 'Asia/Tokyo',
        },
      },
      shell_exec: {
        url: new URL(config.sandboxMcpUrl),
      },
      rss: {
        command: './bin/rss',
        args: [],
        env: {
          RSS_CONFIG_PATH: `${config.dataDir}/rss.yaml`,
          TZ: process.env.TZ ?? 'Asia/Tokyo',
        },
      },
      weather: {
        command: './bin/weather',
        args: [],
        env: {
          LOCATION_PATH: process.env.WEATHER_LOCATION_PATH ?? '3/16/4410/13104/', // Default to Shinjuku, Tokyo
        },
      },
      metrics: {
        command: './bin/metrics',
        args: [],
        env: {
          PROMETHEUS_BASE_URL: process.env.METRICS_QUERIES ?? 'http://localhost:9090',
          TZ: process.env.TZ ?? 'Asia/Tokyo',
          METRICS_QUERIES: 'temperature=sensor_dht_temperature\nhumidity=sensor_dht_humidity\nco2=sensor_mhz19_co2',
          PROMETHEUS_BASIC_AUTH_USERNAME: process.env.PROMETHEUS_BASIC_AUTH_USERNAME ?? '',
          PROMETHEUS_BASIC_AUTH_PASSWORD: process.env.PROMETHEUS_BASIC_AUTH_PASSWORD ?? '',
        },
      },
    },
  })
}

export type MCPAuthHandler = (userId: string, server: string, url: string) => Promise<void>

export function getUserSpecificMCP(config: ConfigService, clientId: string): MCPClient {
  return new MCPClient({
    id: clientId,
    servers: {
      scheduler: {
        command: './bin/scheduler',
        args: [],
        env: {
          DATA_DIR: `${config.dataDir}/${clientId}__scheduler`,
          SCHEDULER_LOOP_INTERVAL_MS: '1000',
          TZ: process.env.TZ ?? 'Asia/Tokyo',
        },
      },
    },
  })
}
