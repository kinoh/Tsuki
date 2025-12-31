import { AgentService } from './agentService'
import { logger } from '../logger'

type SensoryPollerConfig = {
  userIds: string[]
  pollSeconds: number
  immediate?: boolean
}

export type SensorySample = {
  source: string
  text: string
  timestamp: Date
}

export interface SensoryFetcher {
  identifier(): string
  fetch(): Promise<SensorySample | null>
}

/**
 * Lightweight internal sensory service.
 * Polls on an interval and forwards sensory text to AgentService for the given users.
 * Intentional design: no strict dedupe; repeated inputs may surface new facets in the router.
 */
export class SensoryService {
  private timer: NodeJS.Timeout | null = null
  private readonly pollSeconds: number
  private readonly userIds: string[]
  private readonly immediate: boolean
  private readonly fetchers: SensoryFetcher[] = []
  private readonly lastValues: Map<string, string> = new Map()

  constructor(
    private readonly agentService: AgentService,
    config: SensoryPollerConfig,
  ) {
    this.pollSeconds = Math.max(1, config.pollSeconds)
    this.userIds = config.userIds.filter(Boolean)
    this.immediate = config.immediate ?? false
  }

  registerFetcher(fetcher: SensoryFetcher): typeof this {
    this.fetchers.push(fetcher)
    return this
  }

  start(): void {
    if (this.timer || this.userIds.length === 0) {
      return
    }

    this.timer = setInterval(() => {
      this.emitSensory().catch((err: unknown) => {
        logger.error({ err }, 'SensoryService: poll error')
      })
    }, this.pollSeconds * 1000)

    if (this.immediate) {
      this.emitSensory().catch((err: unknown) => {
        logger.error({ err }, 'SensoryService: poll error')
      })
    }
  }

  stop(): void {
    if (this.timer) {
      clearInterval(this.timer)
      this.timer = null
    }
  }

  [Symbol.dispose](): void {
    this.stop()
  }

  private async emitSensory(): Promise<void> {
    const samples = await this.fetchSamples()

    for (const sample of samples) {
      await this.emitSensorySample(sample)
    }
  }

  private async emitSensorySample(sample: SensorySample): Promise<void> {
    const message = `[source:${sample.source}] [time:${sample.timestamp.toISOString()}] ${sample.text}`
    await Promise.all(
      this.userIds.map((userId) =>
        this.agentService.processMessage(userId, {
          userId: 'system',
          type: 'sensory',
          text: message,
        }),
      ),
    )
  }

  private async fetchSamples(): Promise<SensorySample[]> {
    const samples: SensorySample[] = []

    for (const fetcher of this.fetchers) {
      try {
        const sample = await fetcher.fetch()
        if (sample) {
          if (!this.isDuplicate(fetcher.identifier(), sample)) {
            samples.push(sample)
          }
        }
      } catch (err) {
        logger.error({ err }, 'SensoryService: fetcher error')
      }
    }

    logger.info({ count: samples.length }, 'SensoryService: samples fetched')

    return samples
  }

  private isDuplicate(fetcherId: string, sample: SensorySample): boolean {
    const lastValue = this.lastValues.get(fetcherId)
    if (lastValue === sample.text) {
      return true
    }
    this.lastValues.set(fetcherId, sample.text)
    return false
  }
}
