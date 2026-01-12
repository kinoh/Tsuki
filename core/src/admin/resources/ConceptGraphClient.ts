import * as neo4j from 'neo4j-driver'
import { ConfigService } from '../../configService'
import { logger } from '../../logger'

export type ConceptGraphRow = Record<string, unknown>

export class ConceptGraphClient {
  private driver: neo4j.Driver

  constructor(config: ConfigService) {
    const user = process.env.MEMGRAPH_USER
    const password = process.env.MEMGRAPH_PASSWORD ?? ''
    const hasUser = typeof user === 'string' && user.trim() !== ''
    const auth = hasUser ? neo4j.auth.basic(user, password) : undefined
    this.driver = neo4j.driver(config.memgraphUri, auth)
  }

  static asNumber(value: unknown): number {
    if (neo4j.isInt(value)) {
      return value.toNumber()
    }
    if (typeof value === 'number') {
      return value
    }
    if (typeof value === 'string') {
      const parsed = Number(value)
      return Number.isFinite(parsed) ? parsed : 0
    }
    return 0
  }

  async query<T extends ConceptGraphRow>(cypher: string, params: Record<string, unknown> = {}): Promise<T[]> {
    const session = this.driver.session()
    try {
      const result = await session.run(cypher, params)
      return result.records.map(record => record.toObject() as T)
    } catch (err) {
      logger.error({ err, cypher }, 'Concept graph query failed')
      return []
    } finally {
      await session.close()
    }
  }
}
