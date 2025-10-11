import { MessageSender } from './activeuser'
import { ResponseMessage } from './message'

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
      console.log(`[${principalUserId}]: ${JSON.stringify(message)}`)
    } else {
      console.log(`[${principalUserId}]:`)
      console.log(`  timestamp: ${message.timestamp}`)
      console.log(`  role: ${message.role}`)
      console.log(`  user: ${message.user}`)
      console.log('  chat:')
      message.chat.forEach((chat) => {
        console.log(`    ${chat}`)
      })
    }

    return Promise.resolve()
  }
}
