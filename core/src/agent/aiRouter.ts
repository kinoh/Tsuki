import { generateText } from 'ai'
import { openai } from '@ai-sdk/openai'
import { MessageInput } from './activeuser'
import { MessageRouter, RouteDecision } from './router'
import { UserContext } from './userContext'

// Router prompt is public and only handles routing, not persona.
const ROUTER_APPEND_INSTRUCTIONS = `
You are a routing filter.
- Decide whether the assistant should respond.
- If the message is spam, empty, or just acknowledgement (e.g., "了解", "thanks", "ok"), choose skip.
- If the message contains a user question, task, or anything that might need a reply, choose respond.
- Output only one word: respond or skip.
`.trim()

export class AIRouter implements MessageRouter {
  constructor(
    private readonly model: string,
    private readonly baseInstructions: string,
  ) {}

  async route(input: MessageInput, _ctx: UserContext): Promise<RouteDecision> {
    const prompt = `${this.baseInstructions}\n\n${ROUTER_APPEND_INSTRUCTIONS}\n\nUser message:\n${input.text ?? ''}`.trim()

    const { text } = await generateText({
      model: openai(this.model),
      prompt,
    })

    const normalized = text.toLowerCase().includes('respond') ? 'respond' : 'skip'

    return { action: normalized as RouteDecision['action'] }
  }
}
