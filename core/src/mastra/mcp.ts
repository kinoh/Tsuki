import { MCPClient } from '@mastra/mcp'

// Use same data directory for MCP server data
const dataDir = process.env.DATA_DIR ?? './data'

export const mcp = new MCPClient({
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
