
import { Mastra } from '@mastra/core/mastra'
import { PinoLogger } from '@mastra/loggers'
import { LibSQLStore } from '@mastra/libsql'
import { mkdirSync } from 'fs'
import { tsuki } from './agents/tsuki'

// Initialize data directory
const dataDir = process.env.DATA_DIR ?? './data'
mkdirSync(dataDir, { recursive: true })

export const mastra = new Mastra({
  workflows: {},
  agents: { tsuki },
  storage: new LibSQLStore({
    url: `file:${dataDir}/mastra.db`,
  }),
  logger: new PinoLogger({
    name: 'Mastra',
    level: 'info',
  }),
})
