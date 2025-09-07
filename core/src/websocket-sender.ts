import { WebSocket } from 'ws'
import type { MessageSender } from './agent-service'
import type { ResponseMessage } from './message'

export class WebSocketSender implements MessageSender {
  private connections = new Map<string, WebSocket>()

  addConnection(userId: string, ws: WebSocket): void {
    this.connections.set(userId, ws)
  }

  removeConnection(userId: string): void {
    this.connections.delete(userId)
  }

  sendMessage(userId: string, message: ResponseMessage): Promise<void> {
    const ws = this.connections.get(userId)
    if (ws && ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(message))
    }
    return Promise.resolve()
  }
}
