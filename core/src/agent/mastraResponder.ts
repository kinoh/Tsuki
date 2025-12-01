import type { Agent as MastraAgent } from '@mastra/core'
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
    const runtimeContext = ctx.getRuntimeContext()
    runtimeContext.set('memory', memory)
    const toolsets = await ctx.getToolsets()

    const response = await this.agent.generate(
      [{ role: 'user', content: contentParts }],
      {
        memory: {
          resource: ctx.userId,
          thread: threadId,
          options: { lastMessages: 20 },
        },
        runtimeContext,
        toolsets,
      },
    )

    await this.usage.recordUsage(response, threadId, ctx.userId, this.agent.name)

    return {
      role: 'assistant',
      user: this.agent.name,
      chat: [response.text],
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
