import { MessageInput } from './activeuser'

export type RouteAction = 'respond' | 'ignore'

export interface RouteDecision {
  action: RouteAction
}

export interface MessageRouter {
  route(input: MessageInput): Promise<RouteDecision>
}
