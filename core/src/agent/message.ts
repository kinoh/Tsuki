import type { MastraDBMessage, MastraMessageContentV2 } from '@mastra/core/agent/message-list'
import type { TextUIPart, ReasoningUIPart, ToolInvocationUIPart, SourceUIPart, FileUIPart, StepStartUIPart } from '@ai-sdk/ui-utils'

// User message serialized as text part
export interface UserTextMessage {
  user: string
  text: string
  timestamp: string
}

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

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
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
  const replacer = (_key: string, val: unknown): unknown => {
    if (typeof val === 'bigint') {
      return val.toString()
    }
    if (val !== null && typeof val === 'object') {
      const obj = val
      if (seen.has(obj)) {
        return '[Circular]'
      }
      seen.add(obj)
    }
    return val
  }
  const serialized = JSON.stringify(value, replacer)
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
    const invocation = part.toolInvocation as
      | { state?: unknown; result?: unknown; args?: unknown }
      | { state?: unknown }
      | undefined
    const resultPayload = invocation && 'result' in invocation ? invocation.result : undefined
    if (isToolErrorResult(resultPayload)) {
      label = `${label} (error)`
    }
    if (options.traceTools === true) {
      const state = typeof invocation?.state === 'string' ? invocation.state : ''
      const argsPayload = invocation && 'args' in invocation ? invocation.args : undefined
      const payload = state === 'result' ? resultPayload : argsPayload
      const json = safeJsonStringify(payload)
      if (json !== null) {
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
    if (options.traceTools === true) {
      const state = (part as { state?: unknown }).state
      const isResult = state === 'output-available'
      const payload = isResult
        ? outputPayload
        : (part as { input?: unknown }).input
      const json = safeJsonStringify(payload)
      if (json !== null) {
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
        .filter((detail): detail is { type: 'text'; text: string } => {
          if (!isPlainObject(detail)) {
            return false
          }
          return detail.type === 'text' && typeof detail.text === 'string'
        })
        .map(detail => detail.text)
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
  const parts = Array.isArray(content.parts)
    ? content.parts.filter(isMessageContentPart)
    : []
  return extractTextParts(parts, options)
}

function isMessageContentPart(value: unknown): value is MessageContentPart {
  return isPlainObject(value) && typeof value.type === 'string'
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
