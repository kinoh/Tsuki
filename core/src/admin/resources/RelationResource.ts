import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import * as neo4j from 'neo4j-driver'
import { ConceptGraphClient } from './ConceptGraphClient'

interface RelationEntry {
  id: string
  from: string
  to: string
  type: string
  weight: number
}

type RelationIdPayload = {
  from: string
  to: string
  type: string
}

class RelationProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'number' = 'string',
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
    return ['from', 'to', 'type', 'weight'].includes(this.propertyName)
  }

  isId(): boolean {
    return this.propertyName === 'id'
  }
}

class RelationRecord extends BaseRecord {
  constructor(private readonly relation: RelationEntry, resource: BaseResource) {
    super(relation, resource)
  }

  id(): string {
    return this.relation.id
  }
}

function encodeRelationId(payload: RelationIdPayload): string {
  return Buffer.from(JSON.stringify(payload)).toString('base64url')
}

function decodeRelationId(id: string): RelationIdPayload | null {
  try {
    const json = Buffer.from(id, 'base64url').toString('utf-8')
    const payload = JSON.parse(json) as RelationIdPayload
    if (!payload.from || !payload.to || !payload.type) {
      return null
    }
    return payload
  } catch {
    return null
  }
}

function renderRelationType(type: string): string {
  switch (type) {
    case 'IS_A':
      return 'is-a'
    case 'PART_OF':
      return 'part-of'
    case 'EVOKES':
      return 'evokes'
    default:
      return type
  }
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

export class RelationResource extends BaseResource {
  constructor(private readonly client: ConceptGraphClient) {
    super()
  }

  id(): string {
    return 'relations'
  }

  properties(): BaseProperty[] {
    return [
      new RelationProperty('id', 'string'),
      new RelationProperty('from', 'string'),
      new RelationProperty('to', 'string'),
      new RelationProperty('type', 'string'),
      new RelationProperty('weight', 'number'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  async count(): Promise<number> {
    const rows = await this.client.query<{ count: unknown }>(
      'MATCH ()-[r]->() RETURN count(r) AS count',
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
    const sortBy = optionsObj?.sort?.sortBy ?? 'from'
    const direction = optionsObj?.sort?.direction ?? 'asc'

    const orderColumn = sortBy === 'to'
      ? 'to'
      : sortBy === 'type'
        ? 'type'
        : sortBy === 'weight'
          ? 'weight'
          : 'from'

    const orderDirection = direction === 'desc' ? 'DESC' : 'ASC'

    const rows = await this.client.query<{
      from: unknown
      to: unknown
      type: unknown
      weight: unknown
    }>(
      `MATCH (a)-[r]->(b)
       RETURN a.name AS from, b.name AS to, type(r) AS type, r.weight AS weight
       ORDER BY ${orderColumn} ${orderDirection}
       SKIP $offset LIMIT $limit`,
      { offset: neo4j.int(offset), limit: neo4j.int(limit) },
    )

    const relations: RelationEntry[] = rows.map(row => {
      const rawType = toText(row.type)
      const from = toText(row.from)
      const to = toText(row.to)
      return {
        id: encodeRelationId({ from, to, type: rawType }),
        from,
        to,
        type: renderRelationType(rawType),
        weight: ConceptGraphClient.asNumber(row.weight),
      }
    })

    return relations.map(relation => new RelationRecord(relation, this))
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    const payload = decodeRelationId(id)
    if (!payload) {
      return null
    }

    const rows = await this.client.query<{
      from: unknown
      to: unknown
      type: unknown
      weight: unknown
    }>(
      'MATCH (a {name: $from})-[r]->(b {name: $to}) WHERE type(r) = $type RETURN a.name AS from, b.name AS to, type(r) AS type, r.weight AS weight',
      { from: payload.from, to: payload.to, type: payload.type },
    )

    if (rows.length === 0) {
      return null
    }

    const row = rows[0]
    const rawType = toText(row.type)
    const from = toText(row.from)
    const to = toText(row.to)
    const relation: RelationEntry = {
      id,
      from,
      to,
      type: renderRelationType(rawType),
      weight: ConceptGraphClient.asNumber(row.weight),
    }

    return new RelationRecord(relation, this)
  }

  create(): Promise<BaseRecord> {
    throw new Error('Relation creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Relation update not allowed via admin panel')
  }

  delete(): Promise<void> {
    throw new Error('Relation deletion not allowed via admin panel')
  }
}
