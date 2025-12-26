import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import type { MastraMemory } from '@mastra/core/memory'
import { TABLE_THREADS } from '@mastra/core/storage'
import { appLogger } from '../../logger'

interface Thread {
  id: string
  resourceId: string
  title: string
  metadata: string | null
  createdAt: string
  updatedAt: string
}

interface LibSQLClient {
  execute: (query: string | { sql: string; args: (string | number)[] }) => Promise<{
    rows: Array<Record<string, string | number>>
  }>
}

class ThreadProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'reference' = 'string',
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
    } catch (error) {
      appLogger.error('Error counting threads', { error })
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

      const threads: Thread[] = result.rows.map((row) => ({
        id: String(row.id),
        resourceId: String(row.resourceId),
        title: String(row.title),
        metadata: typeof row.metadata === 'string' && row.metadata !== '' ? String(row.metadata) : null,
        createdAt: String(row.createdAt),
        updatedAt: String(row.updatedAt),
      }))

      return threads.map(thread => new ThreadRecord(thread, this))
    } catch (error) {
      appLogger.error('Error finding threads', { error })
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
      const thread: Thread = {
        id: String(row.id),
        resourceId: String(row.resourceId),
        title: String(row.title),
        metadata: typeof row.metadata === 'string' && row.metadata !== '' ? String(row.metadata) : null,
        createdAt: String(row.createdAt),
        updatedAt: String(row.updatedAt),
      }

      return new ThreadRecord(thread, this)
    } catch (error) {
      appLogger.error('Error finding thread', { error, threadId: id })
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
    } catch (error) {
      appLogger.error('Error deleting thread', { error, threadId: id })
      throw error
    }
  }
}
