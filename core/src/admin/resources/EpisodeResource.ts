import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import * as neo4j from 'neo4j-driver'
import { ConceptGraphClient } from './ConceptGraphClient'

interface EpisodeEntry {
  name: string
  summary: string
  valence: number
  arousalLevel: number
  accessedAt: Date
}

function toPositiveInt(value: unknown, fallback: number): number {
  const parsed = typeof value === 'number' ? value : Number.parseInt(String(value), 10)
  if (!Number.isFinite(parsed)) {
    return fallback
  }
  return Math.max(0, Math.floor(parsed))
}

function toText(value: unknown): string {
  if (typeof value === 'string') {
    return value
  }
  if (typeof value === 'number' || typeof value === 'boolean' || typeof value === 'bigint') {
    return String(value)
  }
  return ''
}

class EpisodeProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'number' | 'textarea' = 'string',
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
    return ['name', 'accessedAt', 'valence', 'arousalLevel'].includes(this.propertyName)
  }

  isId(): boolean {
    return this.propertyName === 'name'
  }
}

class EpisodeRecord extends BaseRecord {
  constructor(private readonly episode: EpisodeEntry, resource: BaseResource) {
    super(episode, resource)
  }

  id(): string {
    return this.episode.name
  }
}

export class EpisodeResource extends BaseResource {
  constructor(private readonly client: ConceptGraphClient) {
    super()
  }

  id(): string {
    return 'episodes'
  }

  properties(): BaseProperty[] {
    return [
      new EpisodeProperty('name', 'string'),
      new EpisodeProperty('summary', 'textarea'),
      new EpisodeProperty('valence', 'number'),
      new EpisodeProperty('arousalLevel', 'number'),
      new EpisodeProperty('accessedAt', 'datetime'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  async count(): Promise<number> {
    const rows = await this.client.query<{ count: unknown }>(
      'MATCH (e:Episode) RETURN count(e) AS count',
    )
    if (rows.length === 0) {
      return 0
    }
    return ConceptGraphClient.asNumber(rows[0].count)
  }

  async find(_filters: unknown, options: unknown): Promise<BaseRecord[]> {
    const optionsObj = options as { limit?: number; offset?: number; sort?: { sortBy?: string; direction?: 'asc' | 'desc' } } | undefined
    const limit = toPositiveInt(optionsObj?.limit, 10)
    const offset = toPositiveInt(optionsObj?.offset, 0)
    const sortBy = optionsObj?.sort?.sortBy ?? 'accessedAt'
    const direction = optionsObj?.sort?.direction ?? 'desc'

    const orderColumn = sortBy === 'name'
      ? 'e.name'
      : sortBy === 'valence'
        ? 'e.valence'
        : sortBy === 'arousalLevel'
          ? 'e.arousal_level'
          : 'e.accessed_at'

    const orderDirection = direction === 'asc' ? 'ASC' : 'DESC'

    const rows = await this.client.query<{
      name: unknown
      summary: unknown
      valence: unknown
      arousal_level: unknown
      accessed_at: unknown
    }>(
      `MATCH (e:Episode)
       RETURN e.name AS name, e.summary AS summary, e.valence AS valence, e.arousal_level AS arousal_level, e.accessed_at AS accessed_at
       ORDER BY ${orderColumn} ${orderDirection}
       SKIP $offset LIMIT $limit`,
      { offset: neo4j.int(offset), limit: neo4j.int(limit) },
    )

    const episodes: EpisodeEntry[] = rows.map(row => ({
      name: toText(row.name),
      summary: toText(row.summary),
      valence: ConceptGraphClient.asNumber(row.valence),
      arousalLevel: ConceptGraphClient.asNumber(row.arousal_level),
      accessedAt: new Date(ConceptGraphClient.asNumber(row.accessed_at)),
    }))

    return episodes.map(episode => new EpisodeRecord(episode, this))
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    const rows = await this.client.query<{
      name: unknown
      summary: unknown
      valence: unknown
      arousal_level: unknown
      accessed_at: unknown
    }>(
      'MATCH (e:Episode {name: $name}) RETURN e.name AS name, e.summary AS summary, e.valence AS valence, e.arousal_level AS arousal_level, e.accessed_at AS accessed_at',
      { name: id },
    )

    if (rows.length === 0) {
      return null
    }

    const row = rows[0]
    const episode: EpisodeEntry = {
      name: toText(row.name),
      summary: toText(row.summary),
      valence: ConceptGraphClient.asNumber(row.valence),
      arousalLevel: ConceptGraphClient.asNumber(row.arousal_level),
      accessedAt: new Date(ConceptGraphClient.asNumber(row.accessed_at)),
    }

    return new EpisodeRecord(episode, this)
  }

  create(): Promise<BaseRecord> {
    throw new Error('Episode creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Episode update not allowed via admin panel')
  }

  delete(): Promise<void> {
    throw new Error('Episode deletion not allowed via admin panel')
  }
}
