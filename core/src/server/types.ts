import { Agent } from '@mastra/core'

export type AppRuntimeContext = {
  instructions: string
}

export type AgentMemory = NonNullable<ReturnType<Agent['getMemory']>>

export interface Thread {
  id: string
  [key: string]: unknown
}

export interface GetMessagesQuery {
  n?: string
  before?: string
}

declare global {
  // eslint-disable-next-line @typescript-eslint/no-namespace
  namespace Express {
    interface Locals {
      agent: Agent
      agentMemory: AgentMemory
    }
  }
}
