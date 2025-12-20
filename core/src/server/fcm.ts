import { cert, FirebaseAppError, getApp, initializeApp, ServiceAccount } from 'firebase-admin/app'
import { getMessaging, Messaging, MulticastMessage } from 'firebase-admin/messaging'
import { FCMTokenStorage } from '../storage/fcm'
import { MessageSender } from '../agent/activeuser'
import { ResponseMessage } from '../agent/message'
import { appLogger } from '../logger'

export interface Notification {
  title: string
  body: string
  imageUrl?: string
}

export class FCMManager implements MessageSender {
  private readonly messaging: Messaging

  public constructor(private storage: FCMTokenStorage) {
    try {
      void getApp()
    } catch (e) {
      if (e instanceof FirebaseAppError && e.code === 'app/no-app') {
        this.initialize()
      }
    }

    this.messaging = getMessaging() 
  }

  private initialize(): void {
    appLogger.info('Initializing Firebase app for FCMManager')

    const projectId = process.env.FCM_PROJECT_ID
    if (projectId === undefined) {
      throw new Error('FCM_PROJECT_ID environment variable is not set')
    }

    const serviceAccountKey = process.env.GCP_SERVICE_ACCOUNT_KEY
    if (serviceAccountKey === undefined) {
      throw new Error('GCP_SERVICE_ACCOUNT_KEY environment variable is not set')
    }

    initializeApp({
      credential: cert(JSON.parse(serviceAccountKey) as ServiceAccount),
      projectId,
    })
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
      appLogger.info(`No FCM tokens found for user ${userId}`, { userId })
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
          appLogger.error(`Error sending FCM message for token ${tokens[index]}`, {
            error: response.error,
            token: tokens[index],
            userId,
          })
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
