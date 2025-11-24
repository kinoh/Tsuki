import { RuntimeContext } from '@mastra/core/runtime-context'
import { MCPClient } from '../mastra/mcp'
import type { AgentRuntimeContext } from './activeuser'

// Facade exposing only per-user state needed for response generation.
export interface UserContext {
  readonly userId: string
  readonly mcp: MCPClient | null

  getCurrentThread(): Promise<string>
  loadMemory(): Promise<string>
  getToolsets(): Promise<Record<string, unknown>>
  getRuntimeContext(): RuntimeContext<AgentRuntimeContext>
}
