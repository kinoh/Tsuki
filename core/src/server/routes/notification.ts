import { Request, Response } from 'express'
import { FCMTokenStorage } from '../../storage/fcm'
import { FCMManager } from '../fcm'
import { logger } from '../../logger'
import { RuntimeConfigStore } from '../../runtimeConfig'

interface MutateNotificationTokenPayload {
  token?: string
}

export async function notificationTokenHandler(req: Request, res: Response): Promise<void> {
  try {
    const agentMemory = req.app.locals.agentMemory
    const userId = res.locals.user as string
    const payload = req.body as MutateNotificationTokenPayload
    const tokenStorage = new FCMTokenStorage(agentMemory.storage)

    if (payload.token === undefined || payload.token.trim() === '') {
      logger.warn({ userId }, 'Missing token parameter in request')
      res.status(400).json({ error: 'Missing token parameter' })
      return
    }

    switch (req.method) {
      case 'PUT':
        await tokenStorage.addToken(userId, payload.token)
        break
      case 'DELETE':
        await tokenStorage.removeToken(userId, payload.token)
        break
      default:
        res.status(405).json({ error: 'Method not allowed' })
        return
    }
  } catch (err) {
    logger.error({ err }, 'Error handling notification token')
    res.status(500).json({ error: 'Internal server error' })
    return
  }

  res.status(200).json({ ok: true })
  return
}

export async function notificationTokensHandler(req: Request, res: Response): Promise<void> {
  try {
    const agentMemory = req.app.locals.agentMemory
    const userId = res.locals.user as string
    const tokenStorage = new FCMTokenStorage(agentMemory.storage)

    const tokens = await tokenStorage.getTokens(userId)

    res.status(200).json({ tokens })
  } catch (err) {
    logger.error({ err }, 'Error fetching notification tokens')
    res.status(500).json({ error: 'Internal server error' })
    return
  }
}

export async function notificationTestHandler(req: Request, res: Response): Promise<void> {
  try {
    const agentMemory = req.app.locals.agentMemory
    const userId = res.locals.user as string
    const runtimeConfigStore = req.app.locals.runtimeConfigStore as RuntimeConfigStore

    if (!runtimeConfigStore.get().enableNotification) {
      res.status(409).json({ error: 'Notifications are disabled' })
      return
    }

    const tokenStorage = new FCMTokenStorage(agentMemory.storage)
    // Only for testing purposes, in real usage the FCMManager should be a singleton
    const fcm = new FCMManager(tokenStorage, runtimeConfigStore)

    const notification = {
      title: 'Test Notification',
      body: 'This is a test notification.',
    }

    await fcm.sendNotification(userId, notification)
  } catch (err) {
    logger.error({ err }, 'Error sending test notification')
    res.status(500).json({ error: 'Internal server error' })
    return
  }

  res.status(200).json({ ok: true })
  return
}
