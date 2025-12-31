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

function isToolErrorResult(result: unknown): boolean {
  if (result === null || typeof result !== 'object') {
    return false
  }
  const record = result as Record<string, unknown>
  if (record.isError === true || record.error === true) {
    return true
  }
  if (record.isError === false || record.error === false) {
    return false
  }
  if (record.validationErrors !== undefined || record.cause !== undefined) {
    return true
  }
  const code = typeof record.code === 'string' ? record.code.toUpperCase() : ''
  if (code.includes('ERROR') || code.includes('FAILED')) {
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

function extractTextPart(
  part: MessageContentPart,
  options: TextExtractionOptions = {},
): string | null {
  if (part.type === 'tool-invocation') {
    const toolName = part.toolInvocation?.toolName
    let label = '[tool-invocation]'
    if (typeof toolName === 'string' && toolName.trim() !== '') {
      label = `${label} ${toolName}`
    }
    const resultPayload = part.toolInvocation?.result
    if (isToolErrorResult(resultPayload)) {
      label = `${label} (error)`
    }
    if (options.traceTools) {
      const state = part.toolInvocation?.state
      const payload = state === 'result' ? resultPayload : part.toolInvocation?.args
      const json = safeJsonStringify(payload)
      if (json) {
        label = `${label} ${json}`
      }
    }
    return label
  }

  if (typeof part.type === 'string' && part.type.startsWith('tool-')) {
    let label = `[${part.type}]`
    const outputPayload = (part as { output?: unknown }).output
    if (isToolErrorResult(outputPayload)) {
      label = `${label} (error)`
    }
    if (options.traceTools) {
      const state = (part as { state?: unknown }).state
      const isResult = state === 'output-available'
      const payload = isResult
        ? outputPayload
        : (part as { input?: unknown }).input
      const json = safeJsonStringify(payload)
      if (json) {
        label = `${label} ${json}`
      }
    }
    return label
  }

  switch (part.type) {
    case 'text':
      return part.text

    case 'reasoning': {
      // Extract main reasoning text and any text details
      const raw = part as {
        reasoning?: unknown
        text?: unknown
        details?: unknown
      }
      const reasoningText = typeof raw.reasoning === 'string'
        ? raw.reasoning
        : (typeof raw.text === 'string' ? raw.text : '')
      const details = Array.isArray(raw.details) ? raw.details : []
      const detailTexts = details
        .filter(detail => detail && typeof detail === 'object' && (detail as { type?: unknown }).type === 'text')
        .map(detail => (detail as { text?: unknown }).text)
        .filter(text => typeof text === 'string') as string[]
      const text = [reasoningText, ...detailTexts].filter(item => item.trim() !== '').join('\n')
      if (text.trim() === '') {
        return null
      }
      return `[reasoning]\n${text}`.trim()
    }

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

export function extractTextParts(
  parts: MessageContentPart[],
  options: TextExtractionOptions = {},
): string[] {
  return parts
    .map(part => extractTextPart(part, options))
    .filter((part): part is string => part !== null && part.trim() !== '')
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
