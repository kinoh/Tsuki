import { MastraMemory } from '@mastra/core'

export class ConversationManager {
  private memory: MastraMemory
  private readonly RECENT_UPDATE_THRESHOLD_HOURS = 1

  constructor(memory: MastraMemory) {
    this.memory = memory
  }

  private generateThreadId(userId: string, date: Date): string {
    const dateStr = date.toISOString().split('T')[0] // YYYY-MM-DD形式
    return `${userId}-${dateStr}`
  }

  private getPreviousDayThreadId(userId: string): string {
    const yesterday = new Date()
    yesterday.setDate(yesterday.getDate() - 1)
    return this.generateThreadId(userId, yesterday)
  }

  private async getLastMessageTime(threadId: string): Promise<Date | null> {
    try {
      const result = await this.memory.query({
        threadId,
        selectBy: {
          last: 1,
        },
      })

      if (result.messages.length > 0) {
        const lastMessage = result.messages[0]
        // Check if message object has timestamp information
        // Possible fields: createdAt, timestamp, date, etc.
        if ('createdAt' in lastMessage && typeof lastMessage.createdAt === 'string') {
          const timestamp = lastMessage.createdAt
          return new Date(timestamp)
        }
      }

      return null
    } catch (error) {
      console.warn('Failed to get last message time:', error)
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

  async currentThread(userId: string): Promise<string> {
    const today = new Date()
    const todayId = this.generateThreadId(userId, today)
    const yesterdayId = this.getPreviousDayThreadId(userId)

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
    } catch (error) {
      // Ignore if previous day's thread doesn't exist
      console.debug('Previous day thread not found or error occurred:', error)
    }

    // Return today's thread if no previous day's thread or not recently updated
    return todayId
  }
}
