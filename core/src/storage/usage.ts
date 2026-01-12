import type { LanguageModelUsage } from '@mastra/core/stream'
import { MastraStorage } from '@mastra/core/storage'
import { getClient, LibSQLClient } from './libsql'
import { logger } from '../logger'

export interface UsageData {
  id: string
  threadId: string
  userId: string
  agentName: string
  inputTokens: number | null
  outputTokens: number | null
  totalTokens: number | null
  reasoningTokens: number | null
  cachedInputTokens: number | null
  raw: string | null
  createdAt: string
}

const USAGE_STATS_COLUMN_COUNT = 11

export interface MetricsSummary {
  totalUsage: number
  totalMessages: number
  totalThreads: number
}

export class UsageStorage {
  private readonly client: LibSQLClient

  constructor(storage: MastraStorage) {
    this.client = getClient(storage)
    this.initTable().catch((err: unknown) => {
      logger.error({ err }, 'Failed to initialize usage storage')
    })
  }

  async initTable(): Promise<void> {
    try {
      const tableInfo = await this.client.execute({
        sql: 'PRAGMA table_info(usage_stats)',
        args: [],
      })

      if (tableInfo.rows.length === 0) {
        await this.createUsageTable()
      } else if (tableInfo.rows.length !== USAGE_STATS_COLUMN_COUNT) {
        await this.migrateUsageTable()
      }

      await this.ensureUsageIndexes()

      logger.info('UsageStorage initialized - using LibSQL persistence')
    } catch (err) {
      logger.error({ err }, 'Failed to initialize usage_stats table')
      throw err
    }
  }

  private async createUsageTable(): Promise<void> {
    await this.client.execute(`
      CREATE TABLE IF NOT EXISTS usage_stats (
        id TEXT PRIMARY KEY,
        thread_id TEXT NOT NULL,
        user_id TEXT NOT NULL,
        agent_name TEXT NOT NULL,
        input_tokens INTEGER,
        output_tokens INTEGER,
        total_tokens INTEGER,
        reasoning_tokens INTEGER,
        cached_input_tokens INTEGER,
        raw TEXT,
        created_at DATETIME DEFAULT CURRENT_TIMESTAMP
      )
    `)
  }

  private async migrateUsageTable(): Promise<void> {
    logger.info('Migrating usage_stats schema to LanguageModelUsage fields')
    await this.client.execute('ALTER TABLE usage_stats RENAME TO usage_stats_old')
    await this.createUsageTable()
    await this.client.execute({
      sql: `
        INSERT INTO usage_stats (
          id,
          thread_id,
          user_id,
          agent_name,
          input_tokens,
          output_tokens,
          total_tokens,
          reasoning_tokens,
          cached_input_tokens,
          raw,
          created_at
        )
        SELECT
          id,
          thread_id,
          user_id,
          agent_name,
          prompt_tokens,
          completion_tokens,
          total_tokens,
          NULL,
          NULL,
          NULL,
          created_at
        FROM usage_stats_old
      `,
      args: [],
    })
    await this.client.execute('DROP TABLE usage_stats_old')
  }

  private async ensureUsageIndexes(): Promise<void> {
    await this.client.execute(`
      CREATE INDEX IF NOT EXISTS idx_usage_user_created_at
      ON usage_stats(user_id, created_at)
    `)

    await this.client.execute(`
      CREATE INDEX IF NOT EXISTS idx_usage_thread_id
      ON usage_stats(thread_id)
    `)

    await this.client.execute(`
      CREATE INDEX IF NOT EXISTS idx_usage_created_at
      ON usage_stats(created_at)
    `)
  }

  async recordUsage(
    response: { response: { id?: string }; usage: LanguageModelUsage },
    threadId: string,
    userId: string,
    agentName: string,
  ): Promise<void> {
    if (typeof response.response.id !== 'string') {
      return
    }

    const id = response.response.id
    const usage = response.usage
    const raw = typeof usage.raw === 'undefined' ? null : JSON.stringify(usage.raw)

    try {
      await this.client.execute({
        sql: `
          INSERT INTO usage_stats (
            id,
            thread_id,
            user_id,
            agent_name,
            input_tokens,
            output_tokens,
            total_tokens,
            reasoning_tokens,
            cached_input_tokens,
            raw
          ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        `,
        args: [
          id,
          threadId,
          userId,
          agentName,
          usage.inputTokens ?? null,
          usage.outputTokens ?? null,
          usage.totalTokens ?? null,
          usage.reasoningTokens ?? null,
          usage.cachedInputTokens ?? null,
          raw,
        ],
      })
    } catch (err) {
      logger.error({ err }, 'Failed to record usage')
      throw err
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
    } catch (err) {
      logger.error({ err }, 'Failed to get metrics summary')
      // Return default values on error
      return {
        totalUsage: 0,
        totalMessages: 0,
        totalThreads: 0,
      }
    }
  }
}
