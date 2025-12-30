import type { MastraDBMessage, MastraMessageContentV2 } from '@mastra/core/agent/message-list'
import type { TextUIPart, ReasoningUIPart, ToolInvocationUIPart, SourceUIPart, FileUIPart, StepStartUIPart } from '@ai-sdk/ui-utils'

export interface ResponseMessage {
  role: 'user' | 'assistant' | 'system' | 'tool'
  user: string
  chat: string[]
  timestamp: number
}

export type MessageContentPart = TextUIPart | ReasoningUIPart | ToolInvocationUIPart | SourceUIPart | FileUIPart | StepStartUIPart
export type TextExtractionOptions = {
  traceTools?: boolean
}

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
      const text = [...reasoningTexts, ...detailTexts].join('\n')
      if (text.trim() === '') {
        return null
      }
      return `[reasoning]\n${text}`.trim()
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

function isToolInvocationPart(part: MessageContentPart): part is ToolInvocationUIPart {
  return part.type === 'tool-invocation'
}

function isToolErrorResult(result: unknown): boolean {
  if (result === null || typeof result !== 'object') {
    return false
  }
  const record = result as Record<string, unknown>
  if (record.isError === true || record.error === true) {
    return true
  }
  return false
}

function safeJsonStringify(value: unknown): string | null {
  if (typeof value === 'undefined') {
    return null
  }

  const seen = new WeakSet<object>()
  const serialized = JSON.stringify(value, (_key, val) => {
    if (typeof val === 'bigint') {
      return val.toString()
    }
    if (val !== null && typeof val === 'object') {
      if (seen.has(val)) {
        return '[Circular]'
      }
      seen.add(val)
    }
    return val
  })
  return typeof serialized === 'string' ? serialized : null
}

function extractToolTraceLines(part: ToolInvocationUIPart): string[] {
  const invocation = part.toolInvocation
  if (!invocation || typeof invocation !== 'object') {
    return []
  }

  const toolName = typeof invocation.toolName === 'string' ? invocation.toolName : 'unknown-tool'
  const state = invocation.state

  if (state === 'call') {
    const argsText = safeJsonStringify(invocation.args)
    return argsText ? [argsText] : []
  }

  if (state === 'result') {
    const result = invocation.result
    const isError = isToolErrorResult(result)
    const label = `[tool-result] ${toolName}${isError ? ' (error)' : ''}`.trim()
    const resultText = safeJsonStringify(result)
    return resultText ? [label, resultText] : [label]
  }

  return []
}

export function extractTextParts(
  parts: MessageContentPart[],
  options: TextExtractionOptions = {},
): string[] {
  const lines: string[] = []

  for (const part of parts) {
    const text = extractTextPart(part)
    if (text && text.trim() !== '') {
      lines.push(text)
    }

    if (options.traceTools && isToolInvocationPart(part)) {
      lines.push(...extractToolTraceLines(part))
    }
  }

  return lines
}

export function extractTextContent(
  content: MastraMessageContentV2,
  options: TextExtractionOptions = {},
): string[] {
  return extractTextParts(content.parts, options)
}

export function createResponseMessage(
  message: MastraDBMessage,
  agentName: string,
): ResponseMessage {
  return {
    role: message.role,
    user: message.role === 'user' ? (message.resourceId ?? '?') : agentName,
    chat: extractTextContent(message.content),
    timestamp: Math.floor(message.createdAt.getTime() / 1000),
  }
}
