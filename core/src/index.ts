import express from 'express'
import morgan from 'morgan'
import http from 'http'
import { WebSocketServer } from 'ws'
import { RuntimeContext } from '@mastra/core/di'
import { mastra } from './mastra/index'
import { WebSocketManager } from './websocket'
import { ResponseMessage, createResponseMessage } from './message'
import { MastraMessageV1 } from '@mastra/core'
import { loadPromptFromEnv } from './prompt'

type AppRuntimeContext = {
  instructions: string
}

// Function to create runtime context with encrypted prompt
async function createRuntimeContext(): Promise<RuntimeContext<AppRuntimeContext>> {
  const runtimeContext = new RuntimeContext<AppRuntimeContext>()
  
  try {
    const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
    runtimeContext.set('instructions', instructions)
  } catch (error) {
    console.warn('Failed to load encrypted prompt, using fallback:', error)
    runtimeContext.set('instructions', 'You are a helpful chatting agent.')
  }

  return runtimeContext
}

const agent = mastra.getAgent('tsuki')
const agentMemory = agent.getMemory()

if (!agentMemory) {
  throw new Error('Agent must have memory configured')
}

const app = express()

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

// Routes

app.get('/', (req, res) => {
  res.json({
    'message': 'hello',
  })
})

app.get('/threads', authMiddleware, async (req, res) => {
  const userId = res.locals.user as string

  if (!userId || typeof userId !== 'string' || userId.trim() === '') {
    return res.status(400).json({ error: 'User not authenticated' })
  }
  if (agentMemory === undefined) {
    return res.status(500).json({ error: 'Agent memory not available' })
  }

  res.json({
    'threads': await agentMemory.getThreadsByResourceId({ resourceId: userId }),
  })
})

app.get('/threads/:id', authMiddleware, async (req, res) => {
  try {
    const userId = res.locals.user as string
    const threadId = req.params.id

    if (!userId || typeof userId !== 'string' || userId.trim() === '') {
      return res.status(400).json({ error: 'User not authenticated' })
    }

    // Check if thread exists
    const thread = await agentMemory.getThreadById({ threadId })
    if (!thread) {
      return res.status(404).json({ error: 'Thread not found' })
    }

    // Get messages
    const result = await agentMemory.query({
      threadId,
      selectBy: {
        last: 1000,
      },
    })

    // Convert to ResponseMessage format
    const messages: ResponseMessage[] = result.messages.map(message => {
      const mastraMessage = message as MastraMessageV1
      return createResponseMessage(mastraMessage, agent.name as string, userId)
    })

    res.json({ messages })
  } catch (error) {
    console.error('Error fetching thread messages:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
})

interface GetMessagesQuery {
  n?: string
  before?: string
}

app.get('/messages', authMiddleware, async (req, res) => {
  try {
    const userId = res.locals.user as string
    const query = req.query as GetMessagesQuery
    
    if (!userId || typeof userId !== 'string' || userId.trim() === '') {
      return res.status(400).json({ error: 'User not authenticated' })
    }
    
    if (agentMemory === undefined) {
      return res.status(500).json({ error: 'Agent memory not available' })
    }

    // Parse parameters
    const n = (typeof query.n === 'string' && query.n.trim() !== '') ? parseInt(query.n, 10) : 20
    const before = (typeof query.before === 'string' && query.before.trim() !== '') ? parseInt(query.before, 10) : undefined

    if (isNaN(n) || n <= 0) {
      return res.status(400).json({ error: 'Invalid n parameter' })
    }

    if (before !== undefined && (isNaN(before) || before <= 0)) {
      return res.status(400).json({ error: 'Invalid before parameter' })
    }

    // Get all threads for user
    const threads = await agentMemory.getThreadsByResourceId({ resourceId: userId })
    
    // Filter threads by userId prefix and sort by date part (descending)
    const userThreads = threads
      .filter(thread => thread.id.startsWith(`${userId}-`))
      .sort((a, b) => {
        // Extract YYYYMMDD part and compare (descending)
        const dateA = a.id.substring(userId.length + 1)
        const dateB = b.id.substring(userId.length + 1)
        return dateB.localeCompare(dateA)
      })

    // Filter threads by before parameter if specified
    let filteredThreads = userThreads
    if (before !== undefined) {
      const beforeDate = new Date(before * 1000).toISOString().split('T')[0].replace(/-/g, '')
      
      filteredThreads = userThreads.filter(thread => {
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
      })

      // Convert to ResponseMessage format and add to collection
      let threadMessages: ResponseMessage[] = result.messages.map(message => {
        const mastraMessage = message as MastraMessageV1
        return createResponseMessage(mastraMessage, agent.name as string, userId)
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
})

// Main function to start server with runtime context
async function startServer(): Promise<void> {
  // Create runtime context with encrypted prompt
  const runtimeContext = await createRuntimeContext()

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

startServer().catch(console.error)
