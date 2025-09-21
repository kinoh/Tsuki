import { MastraStorage } from '@mastra/core'
import { getClient, LibSQLClient } from './libsql'

export class FCMTokenStorage {
  private readonly client: LibSQLClient

  constructor(storage: MastraStorage) {
    this.client = getClient(storage)
    this.initTable().catch((error) => {
      console.error('Failed to initialize FCM token storage:', error)
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
    } catch (error) {
      console.error('Error creating fcm_tokens table:', error)
      throw error
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
