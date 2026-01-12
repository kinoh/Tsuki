import type { Agent as MastraAgent } from '@mastra/core/agent'
import { MessageInput, MCPNotificationResourceUpdated } from './activeuser'
import { ResponseMessage, UserTextMessage, extractTextParts, type MessageContentPart } from './message'
import { UsageStorage } from '../storage/usage'
import { UserContext } from './userContext'
import { ConfigService } from '../configService'

export interface Responder {
  respond(input: MessageInput, ctx: UserContext): Promise<ResponseMessage>
  handleNotification?(
    notification: MCPNotificationResourceUpdated,
    ctx: UserContext,
  ): Promise<ResponseMessage>
}

export class MastraResponder implements Responder {
  constructor(
    private readonly agent: MastraAgent,
    private readonly usage: UsageStorage,
    private readonly config: ConfigService,
  ) {}

  async respond(input: MessageInput, ctx: UserContext): Promise<ResponseMessage> {
    const contentParts: Array<
      | { type: 'text'; text: string }
      | { type: 'image'; image: string; mimeType?: string }
    > = []

    if (input.text?.trim()) {
      const t = new Date()
      const timestamp = new Intl.DateTimeFormat('sv-SE', {
        timeZone: this.config.timeZone,
        hour12: false,
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
      }).format(t)

      const textData: UserTextMessage = {
        timestamp,
        user: input.userId,
        text: input.text,
      }
      contentParts.push({ type: 'text', text: JSON.stringify(textData) })
    }

    if (input.images && input.images.length > 0) {
      for (const image of input.images) {
        contentParts.push({
          type: 'image',
          image: image.data.replace(/^data:.*;base64,/, ''),
          mimeType: image.mimeType,
        })
      }
    }

    const threadId = await ctx.getCurrentThread()
    const personality = await ctx.loadPersonality()
    const requestContext = ctx.getRequestContext()
    requestContext.set('personality', personality)
    const toolsets = await ctx.getToolsets()

    const response = await this.agent.generate(
      [{ role: 'user', content: contentParts }],
      {
        memory: {
          resource: ctx.userId,
          thread: threadId,
          options: { lastMessages: 20 },
        },
        requestContext,
        toolsets,
      },
    )

    await this.usage.recordUsage(response, threadId, input.userId, this.agent.name)

    const uiMessages = (response as { response?: { uiMessages?: unknown } }).response?.uiMessages
    const uiChat = buildChatFromUiMessages(uiMessages, this.config.traceTools)
    const chatParts = uiChat ?? splitConcatenatedJsonObjects(response.text ?? '')

    return {
      role: 'assistant',
      user: this.agent.name,
      chat: chatParts,
      timestamp: Math.floor(Date.now() / 1000),
    }
  }

  async handleNotification(
    notification: MCPNotificationResourceUpdated,
    ctx: UserContext,
  ): Promise<ResponseMessage> {
    const synthesized: MessageInput = {
      userId: 'system',
      text: `Received scheduler notification: ${notification.title}`,
    }
    return this.respond(synthesized, ctx)
  }
}

function splitConcatenatedJsonObjects(text: string): string[] {
  const trimmed = text.trim()
  if (!trimmed.startsWith('{')) {
    return [text]
  }

  const parts: string[] = []
  let depth = 0
  let inString = false
  let escape = false
  let segmentStart = -1

  for (let i = 0; i < text.length; i += 1) {
    const char = text[i]

    if (inString) {
      if (escape) {
        escape = false
        continue
      }

      if (char === '\\') {
        escape = true
        continue
      }

      if (char === '"') {
        inString = false
      }

      continue
    }

    if (char === '"') {
      inString = true
      continue
    }

    if (char === '{') {
      if (depth === 0) {
        segmentStart = i
      }
      depth += 1
      continue
    }

    if (char === '}') {
      if (depth > 0) {
        depth -= 1
        if (depth === 0 && segmentStart !== -1) {
          parts.push(text.slice(segmentStart, i + 1))
          segmentStart = -1
        }
      }
    }
  }

  if (depth !== 0 || parts.length <= 1) {
    return [text]
  }

  const normalized = trimmed.replace(/\s+/g, '')
  const combined = parts.join('').replace(/\s+/g, '')
  if (normalized !== combined) {
    return [text]
  }

  for (const part of parts) {
    try {
      const parsed: unknown = JSON.parse(part)
      if (!isPlainObject(parsed)) {
        return [text]
      }
    } catch {
      return [text]
    }
  }

  return parts
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}

function buildChatFromUiMessages(
  uiMessages: unknown,
  traceTools: boolean,
): string[] | null {
  if (!Array.isArray(uiMessages) || uiMessages.length === 0) {
    return null
  }

  const chat: string[] = []

  for (const message of uiMessages) {
    if (!isPlainObject(message)) {
      continue
    }

    const role = typeof message.role === 'string' ? message.role : ''
    if (role !== 'assistant') {
      continue
    }

    const parts = extractUiMessageParts(message)
    if (parts) {
      chat.push(...extractTextParts(parts, { traceTools }))
      continue
    }

    const content = message.content
    if (typeof content === 'string' && content.trim() !== '') {
      chat.push(content)
    }
  }

  return chat.length > 0 ? chat : null
}

function extractUiMessageParts(message: Record<string, unknown>): MessageContentPart[] | null {
  const directParts = message.parts
  if (isMessageContentPartArray(directParts)) {
    return directParts
  }

  const content = message.content
  if (isMessageContentPartArray(content)) {
    return content
  }

  if (isPlainObject(content)) {
    const nestedParts = content.parts
    if (isMessageContentPartArray(nestedParts)) {
      return nestedParts
    }
  }

  return null
}

function isMessageContentPartArray(value: unknown): value is MessageContentPart[] {
  return Array.isArray(value) && value.every(isMessageContentPart)
}

function isMessageContentPart(value: unknown): value is MessageContentPart {
  return isPlainObject(value) && typeof value.type === 'string'
}
