import { RuntimeContext } from '@mastra/core/runtime-context'
import { MCPClient } from '../mastra/mcp'
import type { AgentRuntimeContext } from './activeuser'

// Facade exposing only per-user state needed for response generation.
export interface UserContext {
  readonly userId: string
  readonly mcp: MCPClient | null

  getCurrentThread(): Promise<string>
  loadMemory(): Promise<string>
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  getToolsets(): Promise<Record<string, Record<string, any>>>
  getRuntimeContext(): RuntimeContext<AgentRuntimeContext>
  getSensoryLog(): string
  appendSensory(entry: string): void
}
