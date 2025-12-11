import { AgentService } from './service'

type SensoryPollerConfig = {
  userIds: string[]
  pollSeconds: number
}

type SensorySample = {
  source: string
  text: string
  timestamp: Date
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

  constructor(
    private readonly agentService: AgentService,
    config: SensoryPollerConfig,
  ) {
    this.pollSeconds = Math.max(1, config.pollSeconds)
    this.userIds = config.userIds.filter(Boolean)
  }

  start(): void {
    if (this.timer || this.userIds.length === 0) {
      return
    }

    this.timer = setInterval(() => {
      this.emitSensory().catch((err) => {
        console.error('SensoryService: poll error', err)
      })
    }, this.pollSeconds * 1000)
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
    const sample = await this.fetchSample()
    if (!sample) {
      return
    }

    const message = `[source:${sample.source}] [time:${sample.timestamp.toISOString()}] ${sample.text}`
    await Promise.all(
      this.userIds.map((userId) =>
        this.agentService.processMessage(userId, {
          userId,
          type: 'sensory',
          text: message,
        }),
      ),
    )
  }

  /**
   * Placeholder fetcher: emits a rotating faux headline.
   * Replace with real sensory data retrieval as needed.
   */
  private async fetchSample(): Promise<SensorySample | null> {
    const feeds: Array<{ source: string; items: string[] }> = [
      {
        source: 'RSS demo feed',
        items: [
          'Evening sky turns pink over the city',
          'Local cafe adds a new seasonal latte',
          'A gentle rain starts after a sunny morning',
        ],
      },
      {
        source: 'Daily mood',
        items: [
          'Feeling cozy and curious',
          'A bit sleepy but cheerful',
          'Quietly energized, ready to chat',
        ],
      },
    ]

    const feed = feeds[Math.floor(Math.random() * feeds.length)]
    const text = feed.items[Math.floor(Math.random() * feed.items.length)]

    return {
      source: feed.source,
      text,
      timestamp: new Date(),
    }
  }
}
