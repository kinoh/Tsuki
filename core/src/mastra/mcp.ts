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
    'structured-memory': {
      command: './bin/structured-memory',
      args: [],
      env: {
        DATA_DIR: `${dataDir}/structured_memory`,
        ROOT_TEMPLATE: '# メモ帳\n',
      },
    },
  },
})

// eslint-disable-next-line @typescript-eslint/no-unused-vars
export function getDynamicMCP(clientId: string, onAuth: (server: string, url: string) => Promise<void>): MCPClient {
  return new MCPClient({
    id: clientId,
    servers: {
    },
  })
}
