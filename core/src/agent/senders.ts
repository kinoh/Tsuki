import { MessageSender } from './activeuser'
import { ResponseMessage } from './message'
import { logger } from '../internal/logger'

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
      logger.info(`[${principalUserId}]: ${JSON.stringify(message)}`)
    } else {
      logger.info(`[${principalUserId}]:`)
      logger.info(`  timestamp: ${message.timestamp}`)
      logger.info(`  role: ${message.role}`)
      logger.info(`  user: ${message.user}`)
      logger.info('  chat:')
      message.chat.forEach((chat) => {
        logger.info(`    ${chat}`)
      })
    }

    return Promise.resolve()
  }
}
