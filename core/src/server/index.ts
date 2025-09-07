import express from 'express'
import morgan from 'morgan'
import http from 'http'
import { WebSocketServer } from 'ws'
import { Agent } from '@mastra/core'
import { WebSocketManager } from '../websocket'
import { createAdminRouter } from '../admin/index'
import { setupRoutes } from './routes/index'
import { AgentService } from '../agent'

export async function serve(
  agent: Agent,
  agentService: AgentService,
): Promise<void> {
  const agentMemory = await agent.getMemory()

  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const app = express()

  // Set dependencies in app.locals
  app.locals.agent = agent
  app.locals.agentMemory = agentMemory

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
  app.use(createAdminRouter(agentMemory))

  // Create HTTP server and WebSocket server
  const server = http.createServer(app)
  const wss = new WebSocketServer({ server })
  const wsmanager = new WebSocketManager(agentService)
  
  // Register WebSocketManager as websocket message sender
  agentService.registerMessageSender('websocket', wsmanager)

  wss.on('connection', (ws, req) => {
    wsmanager.handleConnection(ws, req)
  })

  server.listen(2953, () =>
    console.log('\n🚀 Server ready at: http://localhost:2953\n'),
  )
}
