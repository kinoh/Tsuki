import express from 'express'
import { UsageStorage } from '../../storage/usage'
import { logger } from '../../logger'

export async function metricsHandler(req: express.Request, res: express.Response): Promise<void> {
  try {
    const agentMemory = req.app.locals.agentMemory

    // Initialize usage storage with agent's memory storage
    const usageStorage = new UsageStorage(agentMemory.storage)

    const metrics = await usageStorage.getMetricsSummary()

    res.json({
      total_usage: metrics.totalUsage,
      total_messages: metrics.totalMessages,
      total_threads: metrics.totalThreads,
    })
  } catch (err) {
    logger.error({ err }, 'Error fetching metrics')
    res.status(500).json({ error: 'Internal server error' })
  }
}
