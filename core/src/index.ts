import express from 'express'
import morgan from 'morgan'
import http from 'http'
import WebSocket from 'ws'
import { mastra } from './mastra/index'
import { WebSocketManager } from './websocket'
import { ResponseMessage, createResponseMessage } from './message'
import { MastraMessageV1 } from '@mastra/core'

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

// Routes

app.get('/', (req, res) => {
  res.json({
    'message': 'hello',
  })
})

interface GetThreadsRequestBody {
  user: string
}

app.get('/threads', async (req, res) => {
  const body = req.body as GetThreadsRequestBody

  if (body === null) {
    return res.status(400).json({})
  }

  const userId = body.user

  if (!userId) {
    return res.status(400).json({})
  }
  if (agentMemory === undefined) {
    return res.status(404).json({})
  }

  res.json({
    'threads': await agentMemory.getThreadsByResourceId({ resourceId: userId }),
  })
})

interface GetThreadMessagesQuery {
  user?: string
}

app.get('/threads/:id', async (req, res) => {
  try {
    const userId = (req.body as GetThreadMessagesQuery).user
    const threadId = req.params.id

    if (userId === undefined) {
      return res.status(400).json({ error: 'Missing user parameter' })
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
      return createResponseMessage(mastraMessage, agent.name, userId)
    })

    res.json({ messages })
  } catch (error) {
    console.error('Error fetching thread messages:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
})

interface GetMessagesRequestBody {
  user: string
  thread: string
  message: string
}

app.post('/messages', (req, res) => {
  const body = req.body as GetMessagesRequestBody

  if (body === null) {
    return res.status(400).json({})
  }

  const userId = body.user
  const threadId = body.thread
  const message = body.message

  if (!userId || !threadId || !message) {
    return res.status(400).json({})
  }

  res.json(agent.generate([
    { role: 'user', content: message },
  ], {
    memory: {
      resource: userId,
      thread: {
        id: threadId,
        metadata: {
          'date': new Date().toISOString().slice(0, 10),
          'foo': 'bar',
        },
      },
      options: {
        lastMessages: 20,
      },
    },
  }))
})

// WebSocket

const server = http.createServer(app)
const wss = new WebSocket.Server({ server })
const wsmanager = new WebSocketManager(agent)

wss.on('connection', (ws, req) => {
  wsmanager.handleConnection(ws, req)
})

// Run

server.listen(3000, () =>
  console.log(`
🚀 Server ready at: http://localhost:3000
`),
)
