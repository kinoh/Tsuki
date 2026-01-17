import { BaseResource, BaseProperty, BaseRecord, Filter } from 'adminjs'
import type { MastraMemory } from '@mastra/core/memory'
import fetch from 'node-fetch'
import { logger } from '../../internal/logger'
import { ConfigService } from '../../internal/configService'

// Utility to get the auth token from environment variables
function getAuthToken(): string {
  const token = process.env.WEB_AUTH_TOKEN
  if (token === undefined) {
    throw new Error('WEB_AUTH_TOKEN is not configured')
  }
  return token
}

// ResponseMessage interface based on server.ts
interface ResponseMessage {
  id: string,
  role: string,
  user: string,
  chat: string[],
  timestamp: number,
  inputTokens: number | null,
  outputTokens: number | null,
  totalTokens: number | null,
  reasoningTokens: number | null,
  cachedInputTokens: number | null,
}

type ApiMessage = Omit<ResponseMessage, keyof UsageEntry>

interface UsageEntry {
  inputTokens: number | null
  outputTokens: number | null
  totalTokens: number | null
  reasoningTokens: number | null
  cachedInputTokens: number | null
}

interface LibSQLClient {
  execute: (query: string | { sql: string; args: (string | number)[] }) => Promise<{
    rows: Array<Record<string, string | number>>
  }>
}

const EMPTY_USAGE_ENTRY: UsageEntry = {
  inputTokens: null,
  outputTokens: null,
  totalTokens: null,
  reasoningTokens: null,
  cachedInputTokens: null,
}

function toNumberOrNull(value: string | number | null | undefined): number | null {
  if (typeof value === 'number') {
    return Number.isFinite(value) ? value : null
  }
  if (typeof value === 'string' && value.trim() !== '') {
    const parsed = Number(value)
    return Number.isFinite(parsed) ? parsed : null
  }
  return null
}

class MessageProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'richtext' | 'number' = 'string',
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
    return this.propertyName === 'timestamp'
  }

  isId(): boolean {
    return this.propertyName === 'id'
  }
}

class MessageRecord extends BaseRecord {
  constructor(private readonly message: ResponseMessage, resource: BaseResource) {
    const params = {
      ...message,
      chat: message.chat.join('\n'),
      timestamp: new Date(message.timestamp * 1000),
    }
    super(params, resource)
  }

  id(): string {
    return this.message.id
  }
}

export class MessageResource extends BaseResource {
  private readonly apiBaseUrl: string

  constructor(
    private readonly config: ConfigService,
    private readonly agentMemory: MastraMemory,
  ) {
    super()
    this.apiBaseUrl = `http://localhost:${this.config.serverPort}`
  }

  private get client(): LibSQLClient {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any, @typescript-eslint/no-unsafe-member-access
    return (this.agentMemory.storage as any).client as LibSQLClient
  }

  id(): string {
    return 'messages'
  }

  properties(): BaseProperty[] {
    return [
      new MessageProperty('id', 'string'),
      new MessageProperty('role', 'string'),
      new MessageProperty('user', 'string'),
      new MessageProperty('chat', 'string'),
      new MessageProperty('timestamp', 'datetime'),
      new MessageProperty('totalTokens', 'number'),
      new MessageProperty('inputTokens', 'number'),
      new MessageProperty('outputTokens', 'number'),
      new MessageProperty('reasoningTokens', 'number'),
      new MessageProperty('cachedInputTokens', 'number'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  private async fetchMessages(threadId: string): Promise<ResponseMessage[]> {
    try {
      const token = getAuthToken()
      const response = await fetch(`${this.apiBaseUrl}/threads/${threadId}`, {
        headers: { 'Authorization': `admin:${token}` },
      })

      if (!response.ok) {
        logger.error(
          {
            status: response.status,
            threadId,
          },
          'API request failed',
        )
        return []
      }

      const data = await response.json() as { messages: ApiMessage[] }
      let usageEntries: UsageEntry[] = []
      try {
        usageEntries = await this.fetchUsageEntries(threadId)
      } catch (err) {
        logger.error({ err, threadId }, 'Error fetching usage entries')
      }
      let usageIndex = 0

      for (let i = 0; i < data.messages.length; i++) {
        const baseMessage = data.messages[i]
        const messageWithId = {
          ...baseMessage,
          id: `${threadId}-${i.toString().padStart(3, '0')}`,
        }
        if (baseMessage.role === 'assistant') {
          const usage = usageEntries[usageIndex] ?? EMPTY_USAGE_ENTRY
          data.messages[i] = { ...messageWithId, ...usage }
          usageIndex += 1
        } else {
          data.messages[i] = { ...messageWithId, ...EMPTY_USAGE_ENTRY }
        }
      }

      return data.messages as ResponseMessage[]
    } catch (err) {
      logger.error({ err, threadId }, 'Error fetching messages from API')
      return []
    }
  }

  private async fetchUsageEntries(threadId: string): Promise<UsageEntry[]> {
    const result = await this.client.execute({
      sql: `
        SELECT
          input_tokens,
          output_tokens,
          total_tokens,
          reasoning_tokens,
          cached_input_tokens
        FROM usage_stats
        WHERE thread_id = ?
        ORDER BY created_at ASC
      `,
      args: [threadId],
    })

    return result.rows.map(row => ({
      inputTokens: toNumberOrNull(row.input_tokens),
      outputTokens: toNumberOrNull(row.output_tokens),
      totalTokens: toNumberOrNull(row.total_tokens),
      reasoningTokens: toNumberOrNull(row.reasoning_tokens),
      cachedInputTokens: toNumberOrNull(row.cached_input_tokens),
    }))
  }

  async count(filter: Filter): Promise<number> {
    const threadId = filter.get('id')
    if (!threadId) {
      return 0
    }

    const messages = await this.fetchMessages(threadId.value as string)
    return messages.length
  }

  async find(filter: Filter, options: { limit?: number; offset?: number }): Promise<BaseRecord[]> {
    const threadId = filter.get('id')
    if (!threadId) {
      return []
    }

    const messages = await this.fetchMessages(threadId.value as string)
    const sortedMessages = messages.sort((a, b) => a.id.localeCompare(b.id))

    const limit = options?.limit ?? 10
    const offset = options?.offset ?? 0
    const paginatedMessages = sortedMessages.slice(offset, offset + limit)

    return paginatedMessages.map(message => new MessageRecord(message, this))
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    const [threadId, messageId] = id.split(RegExp('-(?=\\d+$)'))
    if (!threadId || !messageId) {
      return null
    }

    const messages = await this.fetchMessages(threadId)
    const index = Number.parseInt(messageId, 10)
    if (!Number.isFinite(index)) {
      return null
    }
    if (index < 0 || index >= messages.length) {
      return null
    }
    const message = messages[index]

    return new MessageRecord(message, this)
  }

  create(): Promise<BaseRecord> {
    throw new Error('Message creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Message update not allowed via admin panel')
  }

  delete(): Promise<void> {
    throw new Error('Message deletion not allowed via admin panel')
  }
}
