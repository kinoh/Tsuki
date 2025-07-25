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
