import { MastraInstance } from './mastra/index'
import { serve } from './server/index'
import { createAgentService } from './agent/agentService'
import { UsageStorage } from './storage/usage'
import { FCMManager } from './server/fcm'
import { FCMTokenStorage } from './storage/fcm'
import { SensoryService } from './agent/sensoryService'
import { McpSensory } from './agent/sensories/mcpSensory'
import { ConfigService } from './internal/configService'
import { RuntimeConfigStore } from './internal/runtimeConfig'
import { logger } from './internal/logger'

// Main function to start server with runtime context
async function main(): Promise<void> {
  logger.info('Starting Tsuki Core Server...')

  const config = new ConfigService()
  const runtimeConfigStore = new RuntimeConfigStore(config.dataDir)
  await runtimeConfigStore.load()
  using mastraInstance = await MastraInstance.create(config)

  logger.info('Summon agent!')
  const agent = mastraInstance.getAgent('tsuki')

  const agentMemory = await agent.getMemory()
  if (!agentMemory) {
    throw new Error('Agent must have memory configured')
  }

  const usageStorage = new UsageStorage(agentMemory.storage)

  logger.info('Creating Agent Service')
  using agentService = await createAgentService(config, agent, agentMemory, usageStorage, mastraInstance.mcp)

  let fcm: FCMManager | undefined = undefined
  if (config.isProduction) {
    logger.info('Setting up FCM Manager')
    const fcmTokenStorage = new FCMTokenStorage(agentMemory.storage)
    fcm = new FCMManager(fcmTokenStorage, runtimeConfigStore)
  }

  const permanentUsers = (process.env.PERMANENT_USERS ?? '')
    .split(',')
    .map((userId) => userId.trim())
    .filter(Boolean)

  logger.info('Starting Agent Service (includes MCP subscriptions)')
  agentService.start(permanentUsers, fcm)

  // Sensory service runs inside core; SENSORY_POLL_SECONDS is interpreted in seconds.
  const sensoryPollSeconds = Number(process.env.SENSORY_POLL_SECONDS ?? '60')
  const sensoryPollImmediate = process.env.SENSORY_POLL_IMMEDIATE === '1'
  using sensoryService = new SensoryService(agentService, {
    userIds: permanentUsers,
    pollSeconds: sensoryPollSeconds,
    immediate: sensoryPollImmediate,
  })
    .registerFetcher(new McpSensory(mastraInstance.mcp, 'rss', 'get_articles', {
    n: 5,
  }))
    .registerFetcher(new McpSensory(mastraInstance.mcp, 'weather', 'get_weather', {
  }))

  if (runtimeConfigStore.get().enableSensory) {
    sensoryService.start()
  }

  runtimeConfigStore.onChange((nextConfig) => {
    if (nextConfig.enableSensory) {
      sensoryService.start()
    } else {
      sensoryService.stop()
    }
  })

  logger.info('Serving...')

  await serve(config, agent, agentService, runtimeConfigStore)
}

main().catch((err: unknown) => {
  logger.error({ err }, 'Unhandled error during startup')
})
