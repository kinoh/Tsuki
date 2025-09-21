import { MastraStorage } from '@mastra/core'
import { getClient, LibSQLClient } from './libsql'

export interface UsageData {
  id: string
  timestamp: number
  threadId: string
  userId: string
  agentName: string
  promptTokens: number
  completionTokens: number
  totalTokens: number
}

export interface MetricsSummary {
  totalUsage: number
  totalMessages: number
  totalThreads: number
}

export class UsageStorage {
  private readonly client: LibSQLClient

  constructor(storage: MastraStorage) {
    this.client = getClient(storage)
    this.initTable().catch((error) => {
      console.error('Failed to initialize usage storage:', error)
    })
  }

  async initTable(): Promise<void> {
    try {
      await this.client.execute(`
        CREATE TABLE IF NOT EXISTS usage_stats (
          id TEXT PRIMARY KEY,
          timestamp INTEGER NOT NULL,
          thread_id TEXT NOT NULL,
          user_id TEXT NOT NULL,
          agent_name TEXT NOT NULL,
          prompt_tokens INTEGER NOT NULL,
          completion_tokens INTEGER NOT NULL,
          total_tokens INTEGER NOT NULL,
          created_at DATETIME DEFAULT CURRENT_TIMESTAMP
        )
      `)

      // Create indexes for efficient queries
      await this.client.execute(`
        CREATE INDEX IF NOT EXISTS idx_usage_user_timestamp 
        ON usage_stats(user_id, timestamp)
      `)

      await this.client.execute(`
        CREATE INDEX IF NOT EXISTS idx_usage_thread_id 
        ON usage_stats(thread_id)
      `)

      await this.client.execute(`
        CREATE INDEX IF NOT EXISTS idx_usage_timestamp 
        ON usage_stats(timestamp)
      `)

      console.log('UsageStorage initialized - using LibSQL persistence')
    } catch (error) {
      console.error('Failed to initialize usage_stats table:', error)
      throw error
    }
  }

  async recordUsage(
    response: { response: { id: string }; usage?: { promptTokens?: number; completionTokens?: number; totalTokens?: number } },
    threadId: string,
    userId: string,
    agentName: string,
  ): Promise<void> {
    if (typeof response.usage === 'undefined') {
      return
    }

    const id = response.response.id
    const timestamp = Math.floor(Date.now() / 1000)

    try {
      await this.client.execute({
        sql: `
          INSERT INTO usage_stats (
            id, timestamp, thread_id, user_id, agent_name, 
            prompt_tokens, completion_tokens, total_tokens
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?)
        `,
        args: [
          id,
          timestamp,
          threadId,
          userId,
          agentName,
          response.usage.promptTokens ?? 0,
          response.usage.completionTokens ?? 0,
          response.usage.totalTokens ?? 0,
        ],
      })
    } catch (error) {
      console.error('Failed to record usage:', error)
      throw error
    }
  }

  async getMetricsSummary(): Promise<MetricsSummary> {
    try {
      const result = await this.client.execute({
        sql: `
          SELECT 
            SUM(total_tokens) as total_usage,
            COUNT(*) as total_messages,
            COUNT(DISTINCT thread_id) as total_threads
          FROM usage_stats
        `,
        args: [],
      })

      const row = result.rows[0]
      if (typeof row === 'undefined') {
        return {
          totalUsage: 0,
          totalMessages: 0,
          totalThreads: 0,
        }
      }

      return {
        totalUsage: Number(row.total_usage) || 0,
        totalMessages: Number(row.total_messages) || 0,
        totalThreads: Number(row.total_threads) || 0,
      }
    } catch (error) {
      console.error('Failed to get metrics summary:', error)
      // Return default values on error
      return {
        totalUsage: 0,
        totalMessages: 0,
        totalThreads: 0,
      }
    }
  }
}
