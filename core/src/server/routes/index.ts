import express from 'express'
import { authMiddleware, internalOnlyMiddleware } from '../middleware/index.js'
import { threadsHandler, threadByIdHandler, messagesHandler } from './threads.js'
import { metricsHandler } from './metrics.js'
import { metadataHandler } from './metadata.js'

function rootHandler(req: express.Request, res: express.Response): void {
  res.json({
    'message': 'hello',
  })
}

export function setupRoutes(app: express.Application): void {
  app.get('/', rootHandler)
  app.get('/threads', authMiddleware, threadsHandler)
  app.get('/threads/:id', authMiddleware, threadByIdHandler)
  app.get('/messages', authMiddleware, messagesHandler)
  app.get('/metrics', internalOnlyMiddleware, metricsHandler)
  app.get('/metadata', authMiddleware, metadataHandler)
}
