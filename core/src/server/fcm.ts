import { initializeApp } from 'firebase-admin/app'
import { getMessaging, Messaging, MulticastMessage } from 'firebase-admin/messaging'
import { FCMTokenStorage } from '../storage/fcm'
import { MessageSender } from '../agent/activeuser'
import { ResponseMessage } from '../agent/message'

export interface Notification {
  title: string
  body: string
  imageUrl?: string
}

export class FCMManager implements MessageSender {
  private readonly messaging: Messaging

  public constructor(private storage: FCMTokenStorage) {
    const projectId = process.env.FCM_PROJECT_ID
    if (projectId === undefined) {
      throw new Error('FCM_PROJECT_ID environment variable is not set')
    }

    initializeApp({
      projectId,
    })
    this.messaging = getMessaging() 
  }

  public async addClient(userId: string, token: string): Promise<void> {
    await this.storage.addToken(userId, token)
  }

  public async removeClient(userId: string, token: string): Promise<void> {
    await this.storage.removeToken(userId, token)
  }

  public async sendNotification(userId: string, data: Notification): Promise<void> {
    const tokens = await this.storage.getTokens(userId)
    if (tokens.length === 0) {
      console.log(`No FCM tokens found for user ${userId}`)
      return
    }

    const message: MulticastMessage = {
      notification: {
        title: data.title,
        body: data.body,
        imageUrl: data.imageUrl,
      },
      tokens,
    }

    const batchResponse = await this.messaging.sendEachForMulticast(message)

    if (batchResponse.failureCount > 0) {
      for (const [index, response] of batchResponse.responses.entries()) {
        if (!response.success) {
          console.error(`Error sending FCM message for token ${tokens[index]}:`, response.error)
        }
      }
    }
  }

  public async sendMessage(principalUserId: string, message: ResponseMessage): Promise<void> {
    const chat = message.chat.join(' ')

    await this.sendNotification(principalUserId, {
      title: 'New Message',
      body: chat.length > 100 ? chat.substring(0, 100) + '...' : chat,
    })
  }
}
