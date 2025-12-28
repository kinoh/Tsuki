import type { MastraMemory } from '@mastra/core/memory'
import type { MastraDBMessage } from '@mastra/core/agent/message-list'
import { logger } from '../logger'

export class ConversationManager {
  private readonly RECENT_UPDATE_THRESHOLD_HOURS = 1
  private readonly THREAD_BOUNDARY_OFFSET_HOUR = 4
  private fixedThreadId: string | null = null

  constructor(
    private memory: MastraMemory,
    private userId: string,
  ) {
  }

  private generateThreadId(date: Date): string {
    const timezone = process.env.TZ ?? 'Asia/Tokyo'
    // Clone date and add offset
    const d = new Date(date.getTime())
    d.setTime(d.getTime() - this.THREAD_BOUNDARY_OFFSET_HOUR * 60 * 60 * 1000)
    const formatter = new Intl.DateTimeFormat('en-CA', {
      timeZone: timezone,
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
    })
    const dateStr = formatter.format(d).replace(/-/g, '')
    return `${this.userId}-${dateStr}`
  }

  private getPreviousDayThreadId(): string {
    const yesterday = new Date()
    yesterday.setDate(yesterday.getDate() - 1)
    return this.generateThreadId(yesterday)
  }

  private async getLastMessageTime(threadId: string): Promise<Date | null> {
    try {
      const result = await this.memory.recall({
        threadId,
        perPage: 1,
        page: 0,
        orderBy: {
          field: 'createdAt',
          direction: 'DESC',
        },
      })

      const lastMessage = result.messages[0]
      return lastMessage?.createdAt ?? null
    } catch (err) {
      logger.warn({ err, threadId }, 'Failed to get last message time')
      return null
    }
  }

  private isRecentlyUpdated(lastMessageTime: Date | null): boolean {
    if (!lastMessageTime) {
      return false
    }

    const now = new Date()
    const thresholdTime = new Date(now.getTime() - this.RECENT_UPDATE_THRESHOLD_HOURS * 60 * 60 * 1000)

    return lastMessageTime > thresholdTime
  }

  public fixThread(id: string): void {
    this.fixedThreadId = id
  }

  async currentThread(): Promise<string> {
    if (this.fixedThreadId !== null) {
      return this.fixedThreadId
    }

    const today = new Date()
    const todayId = this.generateThreadId(today)
    const yesterdayId = this.getPreviousDayThreadId()

    try {
      // Check if previous day's thread exists
      const yesterdayThread = await this.memory.getThreadById({
        threadId: yesterdayId,
      })

      if (yesterdayThread) {
        // Get last message time from previous day's thread
        const lastMessageTime = await this.getLastMessageTime(yesterdayId)

        // Continue previous day's thread if updated within 1 hour
        if (this.isRecentlyUpdated(lastMessageTime)) {
          return yesterdayId
        }
      }
    } catch (err) {
      // Ignore if previous day's thread doesn't exist
      logger.debug({ err, threadId: yesterdayId }, 'Previous day thread not found or error occurred')
    }

    // Return today's thread if no previous day's thread or not recently updated
    return todayId
  }

  async getRecentMessages(limit = 10): Promise<MastraDBMessage[]> {
    const threadId = await this.currentThread()
    try {
      const result = await this.memory.recall({
        threadId,
        perPage: limit,
        page: 0,
        orderBy: {
          field: 'createdAt',
          direction: 'DESC',
        },
      })
      const messages = result.messages ?? []
      return [...messages].reverse()
    } catch (err) {
      logger.warn({ err, threadId }, 'Failed to get recent messages')
      return []
    }
  }
}
