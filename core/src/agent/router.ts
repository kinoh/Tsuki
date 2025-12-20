import { MessageInput } from './activeuser'
import { UserContext } from './userContext'

export type RouteAction = 'respond' | 'ignore'

export interface RouteDecision {
  action: RouteAction
}

export interface MessageRouter {
  route(input: MessageInput, ctx: UserContext): Promise<RouteDecision>
}
