import type { Agent as MastraAgent } from '@mastra/core/agent'
import { MessageInput, MCPNotificationResourceUpdated } from './activeuser'
import { ResponseMessage } from './message'
import { UsageStorage } from '../storage/usage'
import { UserContext } from './userContext'

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
  ) {}

  async respond(input: MessageInput, ctx: UserContext): Promise<ResponseMessage> {
    const contentParts: Array<
      | { type: 'text'; text: string }
      | { type: 'image'; image: string; mimeType?: string }
    > = []

    if (input.text?.trim()) {
      contentParts.push({ type: 'text', text: input.text })
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
    const memory = await ctx.loadMemory()
    const requestContext = ctx.getRequestContext()
    requestContext.set('memory', memory)
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

    await this.usage.recordUsage(response, threadId, ctx.userId, this.agent.name)

    const chatParts = splitConcatenatedJsonObjects(response.text ?? '')

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
      const parsed = JSON.parse(part)
      if (parsed === null || Array.isArray(parsed) || typeof parsed !== 'object') {
        return [text]
      }
    } catch {
      return [text]
    }
  }

  return parts
}
