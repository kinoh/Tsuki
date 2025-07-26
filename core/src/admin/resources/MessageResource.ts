import { BaseResource, BaseProperty, BaseRecord, Filter } from 'adminjs'
import fetch from 'node-fetch'

const API_BASE_URL = 'http://localhost:2953'

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
}

class MessageProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'richtext' = 'string',
  ) {
    super({ path: propertyName, type: propertyType })
  }

  name(): string {
    return this.propertyName
  }

  path(): string {
    return this.propertyName
  }

  type(): string {
    return this.propertyType
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
    const chatAsString = { ...message, chat: message.chat.join('\n') }
    super(chatAsString, resource)
  }


  params(): Record<string, unknown> {
    return {
      id: this.id(),
      role: this.message.role,
      user: this.message.user,
      chat: this.message.chat.join('\n'),
      timestamp: new Date(this.message.timestamp * 1000),
    }
  }

  id(): string {
    return this.message.id
  }
}

export class MessageResource extends BaseResource {
  id(): string {
    return 'messages'
  }

  properties(): BaseProperty[] {
    return [
      new MessageProperty('id', 'string'),
      new MessageProperty('role', 'string'),
      new MessageProperty('user', 'string'),
      new MessageProperty('chat', 'textarea'),
      new MessageProperty('timestamp', 'datetime'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  private async fetchMessages(threadId: string): Promise<ResponseMessage[]> {
    try {
      const token = getAuthToken()
      const response = await fetch(`${API_BASE_URL}/threads/${threadId}`, {
        headers: { 'Authorization': `admin:${token}` },
      })

      if (!response.ok) {
        console.error(`API request failed with status ${response.status}`)
        return []
      }

      const data = await response.json() as { messages: ResponseMessage[] }

      for (let i = 0; i < data.messages.length; i++) {
        data.messages[i].id = `${threadId}-${i}`
      }

      return data.messages
    } catch (error) {
      console.error('Error fetching messages from API:', error)
      return []
    }
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
    const sortedMessages = messages.sort((a, b) => b.timestamp - a.timestamp)

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
    const message = messages[parseInt(messageId)]

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
