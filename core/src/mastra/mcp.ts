import { MCPClient as MastraMCPClient, MCPClientOptions } from '@mastra/mcp'

// Use same data directory for MCP server data
const dataDir = process.env.DATA_DIR ?? './data'

export class MCPClient {
  public readonly client: MastraMCPClient

  constructor(options: MCPClientOptions) {
    this.client = new MastraMCPClient(options)
  }

  [Symbol.dispose](): void {
    console.log('Closing MCP connection...')

    this.client.disconnect().catch((err) => {
      console.error('Error disconnecting MCP client:', err)
    })
  }
}

export function getUniversalMCP(): MCPClient {
  return new MCPClient({
    servers: {
      rss: {
        command: './node_modules/.bin/rss-mcp-lite',
        args: [],
        env: {
          DB_PATH: `${dataDir}/rss_feeds.db`,
          OPML_FILE_PATH: `${dataDir}/rss_feeds.opml`,
        },
      },
    },
  })
}

export type MCPAuthHandler = (userId: string, server: string, url: string) => Promise<void>

export function getUserSpecificMCP(clientId: string): MCPClient {
  return new MCPClient({
    id: clientId,
    servers: {
      'structured-memory': {
        command: './bin/structured-memory',
        args: [],
        env: {
          DATA_DIR: `${dataDir}//${clientId}__structured_memory`,
          ROOT_TEMPLATE: '# メモ帳\n',
        },
      },
      scheduler: {
        command: './bin/scheduler',
        args: [],
        env: {
          DATA_DIR: `${dataDir}/${clientId}__scheduler`,
          SCHEDULER_LOOP_INTERVAL_MS: '1000',
          TZ: 'Asia/Tokyo',
        },
      },
    },
  })
}
