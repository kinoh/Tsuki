import { MCPClient, MultiServerTokenStorage, TokenStorageFactory } from '@mastra/mcp'

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

export function getDynamicMCP(onAuth: (server: string, url: string) => Promise<void>): MCPClient {
  return new MCPClient({
    servers: {
      notion: {
        url: new URL('https://mcp.notion.com/mcp'),
        oauth: {
          onAuthURL: async (authUrl: string) => {
            await onAuth('notion', authUrl)
          },
          tokenStorageOptions: {
            filePath: `${dataDir}/mcp_tokens.json`,
          },
        }
      },
    }
  })
}
