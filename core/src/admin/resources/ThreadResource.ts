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

function toPositiveInt(value: unknown, fallback: number): number {
  const parsed = typeof value === 'number' ? value : Number.parseInt(String(value), 10)
  if (!Number.isFinite(parsed)) {
    return fallback
  }
  return Math.max(0, Math.floor(parsed))
}

const SORT_COLUMN_BY_PROPERTY: Record<string, string> = {
  id: 't.id',
  resourceId: 't.resourceId',
  title: 't.title',
  createdAt: 't.createdAt',
  updatedAt: 't.updatedAt',
  totalTokens: 'totalTokens',
  inputTokens: 'inputTokens',
  outputTokens: 'outputTokens',
  reasoningTokens: 'reasoningTokens',
  cachedInputTokens: 'cachedInputTokens',
}

function isSortableProperty(propertyName: string): boolean {
  return Object.prototype.hasOwnProperty.call(SORT_COLUMN_BY_PROPERTY, propertyName)
}

function toSortDirection(value: unknown): 'asc' | 'desc' {
  return value === 'asc' ? 'asc' : 'desc'
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
    return isSortableProperty(this.propertyName)
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
      const optionsObj = options as {
        limit?: number
        offset?: number
        sort?: { sortBy?: string; direction?: 'asc' | 'desc' }
      } | undefined
      const limit = toPositiveInt(optionsObj?.limit, 10)
      const offset = toPositiveInt(optionsObj?.offset, 0)
      const sortBy = typeof optionsObj?.sort?.sortBy === 'string' ? optionsObj?.sort?.sortBy : 'createdAt'
      const sortColumn = isSortableProperty(sortBy) ? SORT_COLUMN_BY_PROPERTY[sortBy] : SORT_COLUMN_BY_PROPERTY.createdAt
      const sortDirection = toSortDirection(optionsObj?.sort?.direction)

      const result = await this.client.execute({
        sql: `
          SELECT
            t.id,
            t.resourceId,
            t.title,
            t.metadata,
            t.createdAt,
            t.updatedAt,
            COALESCE(SUM(u.total_tokens), 0) AS totalTokens,
            COALESCE(SUM(u.input_tokens), 0) AS inputTokens,
            COALESCE(SUM(u.output_tokens), 0) AS outputTokens,
            COALESCE(SUM(u.reasoning_tokens), 0) AS reasoningTokens,
            COALESCE(SUM(u.cached_input_tokens), 0) AS cachedInputTokens
          FROM ${TABLE_THREADS} t
          LEFT JOIN usage_stats u ON u.thread_id = t.id
          GROUP BY t.id, t.resourceId, t.title, t.metadata, t.createdAt, t.updatedAt
          ORDER BY ${sortColumn} ${sortDirection}
          LIMIT ? OFFSET ?
        `,
        args: [limit, offset],
      })

      if (result.rows.length === 0) {
        return []
      }

      const threads: Thread[] = result.rows.map((row) => ({
        id: String(row.id),
        resourceId: String(row.resourceId),
        title: String(row.title),
        metadata: typeof row.metadata === 'string' && row.metadata !== '' ? String(row.metadata) : null,
        createdAt: String(row.createdAt),
        updatedAt: String(row.updatedAt),
        totalTokens: toNumber(row.totalTokens),
        inputTokens: toNumber(row.inputTokens),
        outputTokens: toNumber(row.outputTokens),
        reasoningTokens: toNumber(row.reasoningTokens),
        cachedInputTokens: toNumber(row.cachedInputTokens),
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
        sql: `
          SELECT
            t.id,
            t.resourceId,
            t.title,
            t.metadata,
            t.createdAt,
            t.updatedAt,
            COALESCE(SUM(u.total_tokens), 0) AS totalTokens,
            COALESCE(SUM(u.input_tokens), 0) AS inputTokens,
            COALESCE(SUM(u.output_tokens), 0) AS outputTokens,
            COALESCE(SUM(u.reasoning_tokens), 0) AS reasoningTokens,
            COALESCE(SUM(u.cached_input_tokens), 0) AS cachedInputTokens
          FROM ${TABLE_THREADS} t
          LEFT JOIN usage_stats u ON u.thread_id = t.id
          WHERE t.id = ?
          GROUP BY t.id, t.resourceId, t.title, t.metadata, t.createdAt, t.updatedAt
        `,
        args: [id],
      })

      if (result.rows.length === 0) {
        return null
      }

      const row = result.rows[0]
      const thread: Thread = {
        id: String(row.id),
        resourceId: String(row.resourceId),
        title: String(row.title),
        metadata: typeof row.metadata === 'string' && row.metadata !== '' ? String(row.metadata) : null,
        createdAt: String(row.createdAt),
        updatedAt: String(row.updatedAt),
        totalTokens: toNumber(row.totalTokens),
        inputTokens: toNumber(row.inputTokens),
        outputTokens: toNumber(row.outputTokens),
        reasoningTokens: toNumber(row.reasoningTokens),
        cachedInputTokens: toNumber(row.cachedInputTokens),
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
