import { Request, Response } from 'express'
import { logger } from '../../logger'
import { RuntimeConfigStore, RuntimeConfig } from '../../runtimeConfig'

function getStore(req: Request): RuntimeConfigStore {
  return req.app.locals.runtimeConfigStore as RuntimeConfigStore
}

function isConfigPayload(payload: unknown): payload is RuntimeConfig {
  if (payload === null || typeof payload !== 'object') {
    return false
  }

  const candidate = payload as Record<string, unknown>
  return typeof candidate.enableNotification === 'boolean' && typeof candidate.enableSensory === 'boolean'
}

export function configGetHandler(req: Request, res: Response): void {
  try {
    const store = getStore(req)
    res.status(200).json(store.get())
  } catch (err) {
    logger.error({ err }, 'Error fetching runtime config')
    res.status(500).json({ error: 'Internal server error' })
  }
}

export async function configPutHandler(req: Request, res: Response): Promise<void> {
  try {
    if (!isConfigPayload(req.body)) {
      res.status(400).json({ error: 'Invalid payload' })
      return
    }

    const store = getStore(req)
    const updated = await store.set(req.body)
    res.status(200).json(updated)
  } catch (err) {
    logger.error({ err }, 'Error updating runtime config')
    res.status(500).json({ error: 'Internal server error' })
  }
}
