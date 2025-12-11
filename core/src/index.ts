import { MastraInstance } from './mastra/index'
import { serve } from './server/index'
import { createAgentService } from './agent/service'
import { UsageStorage } from './storage/usage'
import { FCMManager } from './server/fcm'
import { FCMTokenStorage } from './storage/fcm'
import { SensoryService } from './agent/sensoryService'

// Main function to start server with runtime context
async function main(): Promise<void> {
  using mastraInstance = await MastraInstance.create()
  const agent = mastraInstance.getAgent('tsuki')

  const agentMemory = await agent.getMemory()
  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const usageStorage = new UsageStorage(agentMemory.storage)
  using agentService = await createAgentService(agent, agentMemory, usageStorage)

  const fcmTokenStorage = new FCMTokenStorage(agentMemory.storage)
  const fcm = new FCMManager(fcmTokenStorage)

  const permanentUsers = (process.env.PERMANENT_USERS ?? '')
    .split(',')
    .map((userId) => userId.trim())
    .filter(Boolean)

  // Start AgentService (includes MCP subscriptions)
  agentService.start(permanentUsers, fcm)

  // Sensory service runs inside core; SENSORY_POLL_SECONDS is interpreted in seconds.
  const sensoryPollSeconds = Number(process.env.SENSORY_POLL_SECONDS ?? '60')
  using sensoryService = new SensoryService(agentService, {
    userIds: permanentUsers,
    pollSeconds: sensoryPollSeconds,
  })
  sensoryService.start()

  await serve(agent, agentService)
}

main().catch(console.error)
