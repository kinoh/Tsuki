import { MastraMessageV1 } from '@mastra/core'

export interface ResponseMessage {
  role: 'user' | 'assistant' | 'system' | 'tool'
  user: string
  chat: string[]
  timestamp: number
}

type MessageContentPart =
  { type: 'text', text: string } |
  { type: 'image' } |
  { type: 'file', filename?: string } |
  { type: 'reasoning', text: string } |
  { type: 'redacted-reasoning', data: string } |
  { type: 'tool-call', toolName: string } |
  { type: 'tool-result', toolName: string }

function extractTextPart(part: MessageContentPart): string {
  if (part.type === 'text') {
    return part.text
  }

  // For non-text types, use [type] text format
  const typeLabel = `[${part.type}]`

  if ('text' in part) {
    return `${typeLabel} ${part.text}`
  }

  // Special handling for parts without text property
  switch (part.type) {
    case 'redacted-reasoning':
      return `${typeLabel} ${part.data}`

    case 'tool-call':
    case 'tool-result':
      return `${typeLabel} ${part.toolName}`

    case 'file':
      return part.filename !== undefined ? `${typeLabel} ${part.filename}` : typeLabel

    default:
      return typeLabel
  }
}

function extractTextContent(content: string | MessageContentPart[]): string {
  if (typeof content === 'string') {
    return content
  }

  // Extract text parts from array
  if (Array.isArray(content)) {
    return content
      .map(extractTextPart)
      .join('\n\n')
  }

  // Fallback: JSON stringify
  return JSON.stringify(content)
}

export function createResponseMessage(
  message: MastraMessageV1,
  agentName: string,
  userIdentifier: string,
): ResponseMessage {
  return {
    role: message.role,
    user: message.role === 'user' ? userIdentifier : agentName,
    chat: [extractTextContent(message.content)],
    timestamp: Math.floor(message.createdAt.getTime() / 1000),
  }
}