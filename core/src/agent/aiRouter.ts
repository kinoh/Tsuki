import { generateText } from 'ai'
import { openai } from '@ai-sdk/openai'
import { MessageInput } from './activeuser'
import { MessageRouter, RouteDecision } from './router'
import { UserContext } from './userContext'
import { appLogger } from '../logger'

// Router prompt is public and only handles routing, not persona.
const ROUTER_APPEND_INSTRUCTIONS = `
You are a routing filter. Output exactly one word: respond, ignore, or maybe.
Ignore any persona/tone. Decide only whether the assistant should reply.

あなたは「前意識」モジュールです。context情報を元にsensory情報（ユーザーの状態や環境情報、スケジュール、外部ニュースなどの非意図的な入力）に返答すべきか判断します

<context>
<description>コアモデルのinstruction</description>
{{instruction}}
</context>

<context>
<description>コアモデルのメモリー</description>
{{memory}}
</context>

<context>
<description>メッセージのログ</description>
{{messages}}
</context>

<context>
<description>sensoryのログ</description>
{{sensories}}
</context>

役割: これらの情報から「今、コアモデルが返答・反応すべきかどうか」を判断する

判断の基準:
- ユーザーの興味関心に近いなら積極的に respond
- ユーザーの状態や外部情報が、会話やユーザー体験に影響しそうなら respond
- ユーザーとの関連性が無い情報は ignore
- 迷った場合は、会話の文脈やユーザーの好みを参考に、より親密で自然な体験になる方を選んでください。maybe でも可

出力フォーマット:
- "{decision}: {reason}"
- {decision} : respond / ignore / maybe のいずれか1語
- {reason} : 判断理由を簡潔に説明する文章
`.trim()

export class AIRouter implements MessageRouter {
  constructor(
    private readonly model: string,
    private readonly baseInstructions: string,
    private readonly maxSensoryLog = 50,
    private readonly rateLimitSensoryRespondPerHour = 1,
  ) {}
  private sensoryBuffer: string[] = []
  private lastSensoryRespondTime: number = 0

  private appendSensory(entry: string): void {
    const trimmed = entry.trim()
    if (!trimmed) {
      return
    }
    this.sensoryBuffer.push(trimmed)
    if (this.sensoryBuffer.length > this.maxSensoryLog) {
      this.sensoryBuffer.shift()
    }
  }

  private getSensoryLog(): string {
    return this.sensoryBuffer.join('\n')
  }

  async route(input: MessageInput, ctx: UserContext): Promise<RouteDecision> {
    const kind = input.type ?? 'message'

    // User messages are always forwarded to the responder.
    if (kind === 'message') {
      return { action: 'respond' }
    }

    const history = await ctx.getMessageHistory()
    const memory = await ctx.loadMemory()
    const sensoryLog = this.getSensoryLog() || 'none'

    // Sensory inputs are gated by the router model.
    this.appendSensory(input.text ?? '')

    const messageLog = history.join('\n') || 'none'
    const prompt = `${ROUTER_APPEND_INSTRUCTIONS
      .replaceAll('{{instruction}}', this.baseInstructions)
      .replaceAll('{{memory}}', memory || 'none')
      .replaceAll('{{messages}}', messageLog)
      .replaceAll('{{sensories}}', sensoryLog)}\n\nIncoming sensory:\n${input.text ?? ''}`.trim()

    const { text } = await generateText({
      model: openai(this.model),
      prompt,
    })

    appLogger.debug('Router output', { text })

    const normalizedText = text.toLowerCase()
    const normalized: RouteDecision['action'] =
      normalizedText.includes('respond') || normalizedText.includes('maybe')
        ? 'respond'
        : 'ignore'

    // Rate limit sensory-triggered responses
    if (normalized === 'respond') {
      const now = Date.now()
      const oneHour = 60 * 60 * 1000
      if (now - this.lastSensoryRespondTime < oneHour / this.rateLimitSensoryRespondPerHour) {
        appLogger.info('Router: Sensory response rate-limited', { userId: input.userId })
        return { action: 'ignore' }
      }
      this.lastSensoryRespondTime = now
    }

    return { action: normalized }
  }
}
