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

export function getDynamicMCP(clientId: string, onAuth: (server: string, url: string) => Promise<void>): MCPClient {
  const callbackServerConfig = {
    publicUrl: 'https://tsuki-auth.kinon.pro/oauth/callback',
    host: '0.0.0.0',
    port: 2954,
  }
  if (process.env.NODE_ENV !== 'production') {
    callbackServerConfig.publicUrl = 'http://localhost:2954/oauth/callback'
  }

  return new MCPClient({
    id: clientId,
    servers: {
      notion: {
        url: new URL('https://mcp.notion.com/mcp'),
        oauth: {
          onAuthURL: async (authUrl: string): Promise<void> => {
            await onAuth('notion', authUrl)
          },
          callbackServerConfig,
          tokenStorage: `${dataDir}/mcp_tokens.json`,
        },
      },
    },
  })
}
