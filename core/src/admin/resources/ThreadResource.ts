import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import type { MastraMemory } from '@mastra/core/memory'
import { TABLE_THREADS } from '@mastra/core/storage'
import { logger } from '../../logger'

interface Thread {
  id: string
  resourceId: string
  title: string
  metadata: string | null
  createdAt: string
  updatedAt: string
  totalTokens: number
  inputTokens: number
  outputTokens: number
  reasoningTokens: number
  cachedInputTokens: number
}

interface LibSQLClient {
  execute: (query: string | { sql: string; args: (string | number)[] }) => Promise<{
    rows: Array<Record<string, string | number>>
  }>
}

interface UsageSummary {
  totalTokens: number
  inputTokens: number
  outputTokens: number
  reasoningTokens: number
  cachedInputTokens: number
}

const EMPTY_USAGE_SUMMARY: UsageSummary = {
  totalTokens: 0,
  inputTokens: 0,
  outputTokens: 0,
  reasoningTokens: 0,
  cachedInputTokens: 0,
}

function toNumber(value: string | number | null | undefined): number {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : 0
  }
  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value)
    return Number.isFinite(parsed) ? parsed : 0
  }
  return 0
}

class ThreadProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'reference' | 'number' = 'string',
  ) {
    super({ path: propertyName, type: propertyType })
  }

  name(): string {
    return this.propertyName
  }

  path(): string {
    return this.propertyName
  }

  isEditable(): boolean {
    return false
  }

  isVisible(): boolean {
    return true
  }

  isSortable(): boolean {
    return this.propertyName === 'id'
  }

  isId(): boolean {
    return this.propertyName === 'id'
  }
}

class ThreadRecord extends BaseRecord {
  constructor(private readonly thread: Thread, resource: BaseResource) {
    super(thread, resource)
  }
}

export class ThreadResource extends BaseResource {
  private agentMemory: MastraMemory

  constructor(agentMemory: MastraMemory) {
    super()
    this.agentMemory = agentMemory
  }

  private get client(): LibSQLClient {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any, @typescript-eslint/no-unsafe-member-access
    return (this.agentMemory.storage as any).client as LibSQLClient
  }

  id(): string {
    return 'threads'
  }

  properties(): BaseProperty[] {
    return [
      new ThreadProperty('id', 'string'),
      new ThreadProperty('resourceId', 'string'),
      new ThreadProperty('title', 'string'),
      new ThreadProperty('createdAt', 'datetime'),
      new ThreadProperty('updatedAt', 'datetime'),
      new ThreadProperty('totalTokens', 'number'),
      new ThreadProperty('inputTokens', 'number'),
      new ThreadProperty('outputTokens', 'number'),
      new ThreadProperty('reasoningTokens', 'number'),
      new ThreadProperty('cachedInputTokens', 'number'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  private async fetchUsageSummaries(threadIds: string[]): Promise<Map<string, UsageSummary>> {
    if (threadIds.length === 0) {
      return new Map()
    }

    const placeholders = threadIds.map(() => '?').join(', ')
    const result = await this.client.execute({
      sql: `
        SELECT
          thread_id,
          COALESCE(SUM(total_tokens), 0) AS total_tokens,
          COALESCE(SUM(input_tokens), 0) AS input_tokens,
          COALESCE(SUM(output_tokens), 0) AS output_tokens,
          COALESCE(SUM(reasoning_tokens), 0) AS reasoning_tokens,
          COALESCE(SUM(cached_input_tokens), 0) AS cached_input_tokens
        FROM usage_stats
        WHERE thread_id IN (${placeholders})
        GROUP BY thread_id
      `,
      args: threadIds,
    })

    const summaries = new Map<string, UsageSummary>()
    for (const row of result.rows) {
      const threadId = String(row.thread_id)
      summaries.set(threadId, {
        totalTokens: toNumber(row.total_tokens),
        inputTokens: toNumber(row.input_tokens),
        outputTokens: toNumber(row.output_tokens),
        reasoningTokens: toNumber(row.reasoning_tokens),
        cachedInputTokens: toNumber(row.cached_input_tokens),
      })
    }

    return summaries
  }

  async count(): Promise<number> {
    try {
      const result = await this.client.execute(`SELECT COUNT(*) as count FROM ${TABLE_THREADS}`)

      if (result.rows.length > 0) {
        return Number(result.rows[0].count) || 0
      }
      return 0
    } catch (err) {
      logger.error({ err }, 'Error counting threads')
      return 0
    }
  }

  async find(_filters: unknown, options: unknown): Promise<BaseRecord[]> {
    try {
      const optionsObj = options as { limit?: number; offset?: number } | undefined
      const limit = optionsObj?.limit ?? 10
      const offset = optionsObj?.offset ?? 0

      const result = await this.client.execute({
        sql: `SELECT * FROM ${TABLE_THREADS} ORDER BY createdAt DESC LIMIT ? OFFSET ?`,
        args: [limit, offset],
      })

      if (result.rows.length === 0) {
        return []
      }

      const threadIds = result.rows.map(row => String(row.id))
      const usageSummaries = await this.fetchUsageSummaries(threadIds)

      const threads: Thread[] = result.rows.map((row) => ({
        id: String(row.id),
        resourceId: String(row.resourceId),
        title: String(row.title),
        metadata: typeof row.metadata === 'string' && row.metadata !== '' ? String(row.metadata) : null,
        createdAt: String(row.createdAt),
        updatedAt: String(row.updatedAt),
        ...(usageSummaries.get(String(row.id)) ?? EMPTY_USAGE_SUMMARY),
      }))

      return threads.map(thread => new ThreadRecord(thread, this))
    } catch (err) {
      logger.error({ err }, 'Error finding threads')
      return []
    }
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    try {
      const result = await this.client.execute({
        sql: `SELECT * FROM ${TABLE_THREADS} WHERE id = ?`,
        args: [id],
      })

      if (result.rows.length === 0) {
        return null
      }

      const row = result.rows[0]
      const usageSummaries = await this.fetchUsageSummaries([String(row.id)])
      const thread: Thread = {
        id: String(row.id),
        resourceId: String(row.resourceId),
        title: String(row.title),
        metadata: typeof row.metadata === 'string' && row.metadata !== '' ? String(row.metadata) : null,
        createdAt: String(row.createdAt),
        updatedAt: String(row.updatedAt),
        ...(usageSummaries.get(String(row.id)) ?? EMPTY_USAGE_SUMMARY),
      }

      return new ThreadRecord(thread, this)
    } catch (err) {
      logger.error({ err, threadId: id }, 'Error finding thread')
      return null
    }
  }

  create(): Promise<BaseRecord> {
    throw new Error('Thread creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Thread update not allowed via admin panel')
  }

  async delete(id: string): Promise<void> {
    try {
      await this.agentMemory.deleteThread(id)
    } catch (err) {
      logger.error({ err, threadId: id }, 'Error deleting thread')
      throw err
    }
  }
}
