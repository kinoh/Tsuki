import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import { ConceptGraphClient } from './ConceptGraphClient'

interface ConceptEntry {
  name: string
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

class ConceptProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'number' = 'string',
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

class ConceptRecord extends BaseRecord {
  constructor(private readonly concept: ConceptEntry, resource: BaseResource) {
    super(concept, resource)
  }

  id(): string {
    return this.concept.name
  }
}

export class ConceptResource extends BaseResource {
  constructor(private readonly client: ConceptGraphClient) {
    super()
  }

  id(): string {
    return 'concepts'
  }

  properties(): BaseProperty[] {
    return [
      new ConceptProperty('name', 'string'),
      new ConceptProperty('valence', 'number'),
      new ConceptProperty('arousalLevel', 'number'),
      new ConceptProperty('accessedAt', 'datetime'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  async count(): Promise<number> {
    const rows = await this.client.query<{ count: unknown }>(
      'MATCH (c:Concept) RETURN count(c) AS count',
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
      ? 'c.name'
      : sortBy === 'valence'
        ? 'c.valence'
        : sortBy === 'arousalLevel'
          ? 'c.arousal_level'
          : 'c.accessed_at'

    const orderDirection = direction === 'asc' ? 'ASC' : 'DESC'

    const rows = await this.client.query<{
      name: unknown
      valence: unknown
      arousal_level: unknown
      accessed_at: unknown
    }>(
      `MATCH (c:Concept)
       RETURN c.name AS name, c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at
       ORDER BY ${orderColumn} ${orderDirection}
       SKIP $offset LIMIT $limit`,
      { offset, limit },
    )

    const concepts: ConceptEntry[] = rows.map(row => ({
      name: String(row.name ?? ''),
      valence: ConceptGraphClient.asNumber(row.valence),
      arousalLevel: ConceptGraphClient.asNumber(row.arousal_level),
      accessedAt: new Date(ConceptGraphClient.asNumber(row.accessed_at)),
    }))

    return concepts.map(concept => new ConceptRecord(concept, this))
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    const rows = await this.client.query<{
      name: unknown
      valence: unknown
      arousal_level: unknown
      accessed_at: unknown
    }>(
      'MATCH (c:Concept {name: $name}) RETURN c.name AS name, c.valence AS valence, c.arousal_level AS arousal_level, c.accessed_at AS accessed_at',
      { name: id },
    )

    if (rows.length === 0) {
      return null
    }

    const row = rows[0]
    const concept: ConceptEntry = {
      name: String(row.name ?? ''),
      valence: ConceptGraphClient.asNumber(row.valence),
      arousalLevel: ConceptGraphClient.asNumber(row.arousal_level),
      accessedAt: new Date(ConceptGraphClient.asNumber(row.accessed_at)),
    }

    return new ConceptRecord(concept, this)
  }

  create(): Promise<BaseRecord> {
    throw new Error('Concept creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Concept update not allowed via admin panel')
  }

  delete(): Promise<void> {
    throw new Error('Concept deletion not allowed via admin panel')
  }
}
