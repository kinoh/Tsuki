import express from 'express'
import { authMiddleware, internalOnlyMiddleware } from '../middleware/index'
import { threadsHandler, threadByIdHandler, messagesHandler } from './threads'
import { metricsHandler } from './metrics'
import { metadataHandler } from './metadata'

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
