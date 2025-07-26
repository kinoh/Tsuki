import express from 'express'
import { UsageStorage } from '../../storage/usage.js'

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
  } catch (error) {
    console.error('Error fetching metrics:', error)
    res.status(500).json({ error: 'Internal server error' })
  }
}
