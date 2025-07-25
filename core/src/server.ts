import express from 'express'
import morgan from 'morgan'
import http from 'http'
import { WebSocketServer } from 'ws'
import { RuntimeContext } from '@mastra/core/di'
import { Agent } from '@mastra/core'
import { WebSocketManager } from './websocket'
import { ResponseMessage, createResponseMessage } from './message'
import { MastraMessageV2 } from '@mastra/core'
import { UsageStorage } from './storage/usage'

// Utility function to get Git hash
async function getGitHash(): Promise<string | null> {
  // In Docker environment, get from environment variable
  if (process.env.GIT_HASH !== undefined) {
    return process.env.GIT_HASH
  }

  // In development environment, get from git command
  try {
    const { execSync } = await import('child_process')
    const hash = execSync('git rev-parse HEAD', { encoding: 'utf8' }).trim()
    return hash
  } catch (error) {
    console.warn('Failed to get git hash:', error)
    return null
  }
}

type AppRuntimeContext = {
  instructions: string
}

// Type for agent memory
type AgentMemory = NonNullable<ReturnType<Agent['getMemory']>>

// Type for thread objects
interface Thread {
  id: string
  [key: string]: unknown
}

// Extend Express Application to include our locals
declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Locals {
      agent: Agent
      agentMemory: AgentMemory
    }
  }
}

// Authentication middleware
function authMiddleware(req: express.Request, res: express.Response, next: express.NextFunction): void | express.Response {
  const authHeader = req.headers.authorization

  if (typeof authHeader !== 'string' || authHeader.trim() === '') {
    return res.status(401).json({ error: 'Authorization header required' })
  }

  // Parse "username:token" format from Authorization header
  // Expected format: "username:token" (not "Bearer token")
  const credentials = authHeader
  const colonIndex = credentials.indexOf(':')
  
  if (colonIndex === -1) {
    return res.status(401).json({ error: 'Invalid authorization format. Expected "username:token"' })
  }

  const username = credentials.substring(0, colonIndex)
  const token = credentials.substring(colonIndex + 1)
  
  // Get expected token from environment
  const expectedToken = process.env.WEB_AUTH_TOKEN
  if (typeof expectedToken !== 'string' || expectedToken.trim() === '') {
    return res.status(500).json({ error: 'Server authentication not configured' })
  }

  // Verify token
  if (token !== expectedToken) {
    return res.status(401).json({ error: 'Invalid token' })
  }

  // Inject user into res.locals
  res.locals.user = username
  next()
}

interface GetMessagesQuery {
  n?: string
  before?: string
}

// Route handlers
function rootHandler(req: express.Request, res: express.Response): void {
  res.json({
    'message': 'hello',
  })
}

async function threadsHandler(req: express.Request, res: express.Response): Promise<void> {
  const agentMemory = req.app.locals.agentMemory
  const userId = res.locals.user as string

  if (!userId || typeof userId !== 'string' || userId.trim() === '') {
    res.status(400).json({ error: 'User not authenticated' })
    return
  }

  const threads = await agentMemory.getThreadsByResourceId({ resourceId: userId }) as Thread[]
  res.json({
    'threads': threads,
  })
}

async function threadByIdHandler(req: express.Request, res: express.Response): Promise<void> {
  try {
    const agent = req.app.locals.agent
    const agentMemory = req.app.locals.agentMemory
    const userId = res.locals.user as string
    const threadId = req.params.id

    if (!userId || typeof userId !== 'string' || userId.trim() === '') {
      res.status(400).json({ error: 'User not authenticated' })
      return
    }

    // Check if thread exists
    const thread = await agentMemory.getThreadById({ threadId }) as Thread | null
    if (thread === null || thread === undefined) {
      res.status(404).json({ error: 'Thread not found' })
      return
    }

    // Get messages
    const result = await agentMemory.query({
      threadId,
      selectBy: {
        last: 1000,
      },
    }) as unknown as { messagesV2: MastraMessageV2[] }

    // Convert to ResponseMessage format
    const messages: ResponseMessage[] = result.messagesV2.map((message: MastraMessageV2) => {
      const agentName = agent.name
      return createResponseMessage(message, agentName, userId)
    })

    res.json({ messages })
  } catch (error) {
    console.error('Error fetching thread messages:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
}

async function messagesHandler(req: express.Request, res: express.Response): Promise<void> {
  try {
    const agent = req.app.locals.agent
    const agentMemory = req.app.locals.agentMemory
    const userId = res.locals.user as string
    const query = req.query as GetMessagesQuery
    
    if (!userId || typeof userId !== 'string' || userId.trim() === '') {
      res.status(400).json({ error: 'User not authenticated' })
      return
    }

    // Parse parameters
    const n = (typeof query.n === 'string' && query.n.trim() !== '') ? parseInt(query.n, 10) : 20
    const before = (typeof query.before === 'string' && query.before.trim() !== '') ? parseInt(query.before, 10) : undefined

    if (isNaN(n) || n <= 0) {
      res.status(400).json({ error: 'Invalid n parameter' })
      return
    }

    if (before !== undefined && (isNaN(before) || before <= 0)) {
      res.status(400).json({ error: 'Invalid before parameter' })
      return
    }

    // Get all threads for user
    const threads = await agentMemory.getThreadsByResourceId({ resourceId: userId }) as Thread[]
    
    // Filter threads by userId prefix and sort by date part (descending)
    const userThreads = threads
      .filter((thread: Thread) => thread.id.startsWith(`${userId}-`))
      .sort((a: Thread, b: Thread) => {
        // Extract YYYYMMDD part and compare (descending)
        const dateA = a.id.substring(userId.length + 1)
        const dateB = b.id.substring(userId.length + 1)
        return dateB.localeCompare(dateA)
      })

    // Filter threads by before parameter if specified
    let filteredThreads = userThreads
    if (before !== undefined) {
      const beforeDate = new Date(before * 1000).toISOString().split('T')[0].replace(/-/g, '')
      
      filteredThreads = userThreads.filter((thread: Thread) => {
        const threadDate = thread.id.substring(userId.length + 1)
        return threadDate <= beforeDate
      })
    }

    // Collect messages from threads until we have enough
    const messages: ResponseMessage[] = []
    let remainingCount = n

    for (const thread of filteredThreads) {
      if (messages.length >= n) {
        break
      }

      const result = await agentMemory.query({
        threadId: thread.id,
        selectBy: {
          last: before === undefined ? remainingCount : 1000, // TODO: Fix "before" handling
        },
      }) as unknown as { messagesV2: MastraMessageV2[] }

      // Convert to ResponseMessage format and add to collection
      let threadMessages: ResponseMessage[] = result.messagesV2.map((message: MastraMessageV2) => {
        const agentName = agent.name
        return createResponseMessage(message, agentName, userId)
      })

      if (before !== undefined) {
        threadMessages = threadMessages.filter(message => message.timestamp < before)
      }

      threadMessages.reverse()

      messages.push(...threadMessages)
      remainingCount = n - messages.length
    }

    // Return first n messages
    const responseMessages = messages.slice(0, n)

    responseMessages.reverse()

    res.json({ messages: responseMessages })
  } catch (error) {
    console.error('Error fetching messages:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
}

async function metricsHandler(req: express.Request, res: express.Response): Promise<void> {
  try {
    const agentMemory = req.app.locals.agentMemory
    const userId = res.locals.user as string

    if (!userId || typeof userId !== 'string' || userId.trim() === '') {
      res.status(400).json({ error: 'User not authenticated' })
      return
    }

    // Initialize usage storage with agent's memory storage
    const usageStorage = new UsageStorage(agentMemory.storage)

    const metrics = await usageStorage.getMetricsSummary()

    res.json({
      total_usage: metrics.totalUsage,
      total_messages: metrics.totalMessages,
      total_threads: metrics.totalThreads,
    })
  } catch (error) {
    console.error('Error fetching metrics:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
}

async function metadataHandler(req: express.Request, res: express.Response): Promise<void> {
  try {
    const agent = req.app.locals.agent
    const gitHash = await getGitHash()
    const openaiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'
    const tools = await agent.getTools()
    const mcpTools = Object.keys(tools)

    res.json({
      git_hash: gitHash,
      openai_model: openaiModel,
      mcp_tools: mcpTools,
    })
  } catch (error) {
    console.error('Error fetching metadata:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
}

export function serve(
  agent: Agent,
  runtimeContext: RuntimeContext<AppRuntimeContext>,
): void {
  const agentMemory = agent.getMemory()

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

  // Routes
  app.get('/', rootHandler)
  app.get('/threads', authMiddleware, threadsHandler)
  app.get('/threads/:id', authMiddleware, threadByIdHandler)
  app.get('/messages', authMiddleware, messagesHandler)
  app.get('/metrics', metricsHandler)
  app.get('/metadata', authMiddleware, metadataHandler)

  // Create HTTP server and WebSocket server
  const server = http.createServer(app)
  const wss = new WebSocketServer({ server })
  const wsmanager = new WebSocketManager(agent, runtimeContext)

  wss.on('connection', (ws, req) => {
    wsmanager.handleConnection(ws, req)
  })

  server.listen(2953, () =>
    console.log(`
🚀 Server ready at: http://localhost:2953
`),
  )
}
