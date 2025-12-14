import { generateText } from 'ai'
import { openai } from '@ai-sdk/openai'
import { MessageInput } from './activeuser'
import { MessageRouter, RouteDecision } from './router'

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
<description>メッセージのログ</description>
{{messages}}
</context>

<context>
<description>sensoryのログ</description>
{{sensories}}
</context>

役割: これらの情報から「今、コアモデルが返答・反応すべきかどうか」を判断する

判断の基準例:
- ユーザーの状態や外部情報が、会話やユーザー体験に影響しそうなら respond
- 重要度が低い、または会話の流れに関係しない情報は ignore
- ユーザーの安全・健康・感情に関わる情報は優先して respond
- 迷った場合は、会話の文脈やユーザーの好みを参考に、より親密で自然な体験になる方を選んでください。maybe でも可

出力フォーマット:
- respond / ignore / maybe のいずれか1語のみを返す
`.trim()

export class AIRouter implements MessageRouter {
  constructor(
    private readonly model: string,
    private readonly baseInstructions: string,
    private readonly maxSensoryLog = 20,
  ) {}
  private sensoryBuffer: string[] = []

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

  async route(input: MessageInput): Promise<RouteDecision> {
    const kind = input.type ?? 'message'

    // User messages are always forwarded to the responder.
    if (kind === 'message') {
      return { action: 'respond' }
    }

    // Sensory inputs are gated by the router model.
    this.appendSensory(input.text ?? '')

    const prompt = `${this.baseInstructions}\n\n${ROUTER_APPEND_INSTRUCTIONS}\n\nSensory log:\n${this.getSensoryLog() || 'none'}\n\nIncoming sensory:\n${input.text ?? ''}`.trim()

    const { text } = await generateText({
      model: openai(this.model),
      prompt,
    })

    const normalizedText = text.toLowerCase()
    const normalized: RouteDecision['action'] =
      normalizedText.includes('respond') || normalizedText.includes('maybe')
        ? 'respond'
        : 'ignore'

    return { action: normalized }
  }
}
