import express from 'express'
import type { MastraDBMessage } from '@mastra/core/agent/message-list'
import { ResponseMessage, createResponseMessage } from '../../agent/message'
import { Thread, GetMessagesQuery } from '../types'
import { appLogger } from '../../logger'

export async function threadsHandler(req: express.Request, res: express.Response): Promise<void> {
  const agentMemory = req.app.locals.agentMemory
  const userId = res.locals.user as string

  if (!userId || typeof userId !== 'string' || userId.trim() === '') {
    res.status(400).json({ error: 'User not authenticated' })
    return
  }

  const { threads } = await agentMemory.listThreadsByResourceId({ resourceId: userId })
  res.json({
    'threads': threads,
  })
}

export async function threadByIdHandler(req: express.Request, res: express.Response): Promise<void> {
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
    const result = await agentMemory.recall({
      threadId,
      perPage: 1000,
      page: 0,
      orderBy: {
        field: 'createdAt',
        direction: 'ASC',
      },
    })

    // Convert to ResponseMessage format
    const messages: ResponseMessage[] = result.messages.map((message: MastraDBMessage) => {
      const agentName = agent.name
      return createResponseMessage(message, agentName)
    })

    res.json({ messages })
  } catch (error) {
    appLogger.error('Error fetching thread messages', { error })
    res.status(500).json({ error: 'Internal server error' })
  }
}

export async function messagesHandler(req: express.Request, res: express.Response): Promise<void> {
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
    const { threads } = await agentMemory.listThreadsByResourceId({ resourceId: userId })

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

      const result = await agentMemory.recall({
        threadId: thread.id,
        perPage: before === undefined ? remainingCount : 1000, // TODO: Fix "before" handling
        page: 0,
        orderBy: {
          field: 'createdAt',
          direction: 'ASC',
        },
      })

      // Convert to ResponseMessage format and add to collection
      let threadMessages: ResponseMessage[] = result.messages.map((message: MastraDBMessage) => {
        const agentName = agent.name
        return createResponseMessage(message, agentName)
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
    appLogger.error('Error fetching messages', { error })
    res.status(500).json({ error: 'Internal server error' })
  }
}
