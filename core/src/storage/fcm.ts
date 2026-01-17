import { MastraStorage } from '@mastra/core/storage'
import { getClient, LibSQLClient } from './libsql'
import { logger } from '../internal/logger'

export class FCMTokenStorage {
  private readonly client: LibSQLClient

  constructor(storage: MastraStorage) {
    this.client = getClient(storage)
    this.initTable().catch((err: unknown) => {
      logger.error({ err }, 'Failed to initialize FCM token storage')
    })
  }

  private async initTable(): Promise<void> {
    try {
      await this.client.execute(`
        CREATE TABLE IF NOT EXISTS fcm_tokens (
          user_id TEXT NOT NULL,
          token TEXT NOT NULL,
          created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
          PRIMARY KEY (user_id, token)
        )
      `)
    } catch (err) {
      logger.error({ err }, 'Error creating fcm_tokens table')
      throw err
    }
  }

  public async addToken(userId: string, token: string): Promise<void> {
    await this.client.execute({
      sql: 'INSERT INTO fcm_tokens (user_id, token) VALUES (?, ?) ON CONFLICT(user_id, token) DO NOTHING',
      args: [userId, token],
    })
  }

  public async removeToken(userId: string, token: string): Promise<void> {
    await this.client.execute({
      sql: 'DELETE FROM fcm_tokens WHERE user_id = ? AND token = ?',
      args: [userId, token],
    })
  }

  public async getTokens(userId: string): Promise<string[]> {
    const result = await this.client.execute({
      sql: 'SELECT token FROM fcm_tokens WHERE user_id = ?',
      args: [userId],
    })
    return result.rows.map(row => String(row.token))
  }
}
