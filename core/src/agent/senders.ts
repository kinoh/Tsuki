import { MessageSender } from './activeuser'
import { ResponseMessage } from './message'
import { appLogger } from '../logger'

export class InternalMessageSender implements MessageSender, AsyncIterable<ResponseMessage> {
  private queue: Array<(value: IteratorResult<ResponseMessage>) => void> = []

  constructor(private format: 'json' | 'multiline') {}

  [Symbol.asyncIterator](): AsyncIterator<ResponseMessage> {
    return {
      next: () =>
        new Promise<IteratorResult<ResponseMessage>>(resolve => {
          this.queue.push(resolve)
        }),
    }
  }

  sendMessage(principalUserId: string, message: ResponseMessage): Promise<void> {
    const resolver = this.queue.shift()
    if (resolver) {
      resolver({ value: message, done: false })
    }

    if (this.format === 'json') {
      appLogger.info(`[${principalUserId}]: ${JSON.stringify(message)}`)
    } else {
      appLogger.info(`[${principalUserId}]:`)
      appLogger.info(`  timestamp: ${message.timestamp}`)
      appLogger.info(`  role: ${message.role}`)
      appLogger.info(`  user: ${message.user}`)
      appLogger.info('  chat:')
      message.chat.forEach((chat) => {
        appLogger.info(`    ${chat}`)
      })
    }

    return Promise.resolve()
  }
}
