import { MCPClient } from '@mastra/mcp'

export const mcp = new MCPClient({
  servers: {
    rss: {
      command: 'npx',
      args: [
        '-y',
        'rss-mcp-lite',
      ],
    },
  },
})
