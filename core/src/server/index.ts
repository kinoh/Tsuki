import express from 'express'
import morgan from 'morgan'
import http from 'http'
import { WebSocketServer } from 'ws'
import { Agent } from '@mastra/core/agent'
import { WebSocketManager } from './websocket'
import { createAdminRouter } from '../admin/index'
import { setupRoutes } from './routes/index'
import { AgentService } from '../agent/agentService'
import { ConfigService } from '../internal/configService'
import { RuntimeConfigStore } from '../internal/runtimeConfig'
import { logger } from '../internal/logger'

export async function serve(
  config: ConfigService,
  agent: Agent,
  agentService: AgentService,
  runtimeConfigStore: RuntimeConfigStore,
): Promise<void> {
  const agentMemory = await agent.getMemory()

  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const app = express()

  // Set dependencies in app.locals
  app.locals.agent = agent
  app.locals.agentMemory = agentMemory
  app.locals.runtimeConfigStore = runtimeConfigStore
  app.locals.config = config

  // Middlewares
  app.use(morgan((tokens, req, res) => {
    return JSON.stringify({
      time: tokens.date(req, res, 'iso'),
      method: tokens.method(req, res),
      url: tokens.url(req, res),
      status: tokens.status(req, res),
      response_time: tokens['response-time'](req, res) + '  ms',
      ip: tokens['remote-addr'](req, res),
      ua: tokens['user-agent'](req, res),
    })
  }))

  app.use(express.json())

  // Setup routes
  setupRoutes(app)

  // Add AdminJS routes (with built-in authentication)
  app.use(createAdminRouter(config, agentMemory))

  // Create HTTP server and WebSocket server
  const server = http.createServer(app)
  const wss = new WebSocketServer({ server })
  const wsmanager = new WebSocketManager(agentService)
  
  wss.on('connection', (ws, req) => {
    wsmanager.handleConnection(ws, req)
  })

  server.listen(config.serverPort, () => {
    logger.info(`Server ready at: http://localhost:${config.serverPort}`)
  })

  const gracefulShutdown = (): void => {
    logger.info('Shutting down server...')
    wss.close(() => {
      server.close(() => {
        logger.info('Server closed.')
        process.exit(0)
      })
    })

    // Force shutdown after 5 seconds
    setTimeout(() => {
      logger.error('Forcing shutdown...')
      process.exit(1)
    }, 5000)
  }

  process.on('SIGINT', gracefulShutdown)
  process.on('SIGTERM', gracefulShutdown)

  // Keep the Promise pending until the server is closed
  await new Promise<void>(() => {})
}
