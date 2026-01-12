import { RequestContext } from '@mastra/core/request-context'
import { MCPClient } from '../mastra/mcp'
import type { AgentRuntimeContext } from './activeuser'

// Facade exposing only per-user state needed for response generation.
export interface UserContext {
  readonly userId: string
  readonly mcp: MCPClient | null

  getCurrentThread(): Promise<string>
  getMessageHistory(): Promise<string[]>
  loadPersonality(): Promise<string>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  getToolsets(): Promise<Record<string, Record<string, any>>>
  getRequestContext(): RequestContext<AgentRuntimeContext>
}
