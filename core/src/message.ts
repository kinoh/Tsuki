import { MastraMessageV2 } from '@mastra/core'
import { MastraMessageContentV2 } from '@mastra/core/agent'
import type { TextUIPart, ReasoningUIPart, ToolInvocationUIPart, SourceUIPart, FileUIPart, StepStartUIPart } from '@ai-sdk/ui-utils'

export interface ResponseMessage {
  role: 'user' | 'assistant' | 'system' | 'tool'
  user: string
  chat: string[]
  timestamp: number
}

type MessageContentPart = TextUIPart | ReasoningUIPart | ToolInvocationUIPart | SourceUIPart | FileUIPart | StepStartUIPart

function extractTextPart(part: MessageContentPart): string | null {
  switch (part.type) {
    case 'text':
      return part.text

    case 'reasoning': {
      // Extract main reasoning text and any text details
      const reasoningTexts = [part.reasoning]
      const detailTexts = part.details
        .filter(detail => detail.type === 'text')
        .map(detail => detail.text)
      return [...reasoningTexts, ...detailTexts].join('\n')
    }

    case 'tool-invocation':
      return `[tool-invocation] ${part.toolInvocation.toolName}`

    case 'file':
      return `[file] ${part.mimeType}`

    case 'source':
      return `[source] ${part.source.sourceType}`

    case 'step-start':
      return null

    default: {
      return '[unknown]'
    }
  }
}

function extractTextContent(content: MastraMessageContentV2): string[] {
  return content.parts.map(extractTextPart).filter(part => part !== null)
}

export function createResponseMessage(
  message: MastraMessageV2,
  agentName: string,
  userIdentifier: string,
): ResponseMessage {
  return {
    role: message.role,
    user: message.role === 'user' ? userIdentifier : agentName,
    chat: extractTextContent(message.content),
    timestamp: Math.floor(message.createdAt.getTime() / 1000),
  }
}