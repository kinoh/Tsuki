import AdminJS, { ActionContext, ActionResponse, BaseRecord } from 'adminjs'
import { Router } from 'express'
import * as AdminJSExpress from '@adminjs/express'
import type { MastraMemory } from '@mastra/core/memory'
import { ThreadResource } from './resources/ThreadResource'
import { MessageResource } from './resources/MessageResource'
import { ConceptGraphClient } from './resources/ConceptGraphClient'
import { ConceptResource } from './resources/ConceptResource'
import { EpisodeResource } from './resources/EpisodeResource'
import { RelationResource } from './resources/RelationResource'
import { SandboxMemoryResource } from './resources/SandboxMemoryResource'
import { ConfigService } from '../configService'
import { logger } from '../logger'

export function createAdminJS(config: ConfigService, agentMemory: MastraMemory): AdminJS {
  const conceptGraphClient = new ConceptGraphClient(config)

  const admin = new AdminJS({
    resources: [
      {
        resource: new ThreadResource(agentMemory),
        options: {
          id: 'threads',
          navigation: {
            name: 'Thread Management',
            icon: 'MessageCircle',
          },
          listProperties: ['id', 'resourceId', 'title', 'totalTokens', 'createdAt', 'updatedAt'],
          showProperties: [
            'id',
            'resourceId',
            'title',
            'inputTokens',
            'outputTokens',
            'totalTokens',
            'reasoningTokens',
            'cachedInputTokens',
            'createdAt',
            'updatedAt',
          ],
          actions: {
            new: {
              isVisible: false,
            },
            edit: {
              isVisible: false,
            },
            delete: {
              isVisible: true,
              isAccessible: true,
            },
            show: {
              isVisible: true,
              isAccessible: true,
            },
            list: {
              isVisible: true,
              isAccessible: true,
            },
            viewMessages: {
              actionType: 'record',
              icon: 'List',
              component: false,
              handler: (request: unknown, response: unknown, context: ActionContext): ActionResponse => {
                const record = context.record as BaseRecord
                const threadId = record.params.id as string
                return {
                  redirectUrl: `/admin/resources/messages?filters.id=${threadId}`,
                  record: record.toJSON(),
                }
              },
            },
          },
          sort: {
            sortBy: 'id',
            direction: 'desc' as const,
          },
        },
      },
      {
        resource: new MessageResource(agentMemory),
        options: {
          id: 'messages',
          navigation: {
            name: 'Thread Management',
          },
          listProperties: ['id', 'role', 'user', 'totalTokens', 'chat', 'timestamp'],
          showProperties: [
            'id',
            'role',
            'user',
            'inputTokens',
            'outputTokens',
            'totalTokens',
            'reasoningTokens',
            'cachedInputTokens',
            'chat',
            'timestamp',
          ],
          actions: {
            new: { isVisible: false },
            edit: { isVisible: false },
            delete: { isVisible: false },
          },
          sort: {
            sortBy: 'timestamp',
            direction: 'desc' as const,
          },
        },
      },
      {
        resource: new ConceptResource(conceptGraphClient),
        options: {
          id: 'concepts',
          navigation: {
            name: 'Concept Graph',
            icon: 'Share2',
          },
          listProperties: ['name', 'valence', 'arousalLevel', 'accessedAt'],
          showProperties: ['name', 'valence', 'arousalLevel', 'accessedAt'],
          actions: {
            new: {
              isVisible: false,
            },
            edit: {
              isVisible: false,
            },
            delete: {
              isVisible: false,
            },
            show: {
              isVisible: true,
              isAccessible: true,
            },
            list: {
              isVisible: true,
              isAccessible: true,
            },
          },
          sort: {
            sortBy: 'accessedAt',
            direction: 'desc' as const,
          },
        },
      },
      {
        resource: new EpisodeResource(conceptGraphClient),
        options: {
          id: 'episodes',
          navigation: {
            name: 'Concept Graph',
            icon: 'Share2',
          },
          listProperties: ['name', 'summary', 'valence', 'arousalLevel', 'accessedAt'],
          showProperties: ['name', 'summary', 'valence', 'arousalLevel', 'accessedAt'],
          actions: {
            new: { isVisible: false },
            edit: { isVisible: false },
            delete: { isVisible: false },
          },
          sort: {
            sortBy: 'accessedAt',
            direction: 'desc' as const,
          },
        },
      },
      {
        resource: new RelationResource(conceptGraphClient),
        options: {
          id: 'relations',
          navigation: {
            name: 'Concept Graph',
            icon: 'Share2',
          },
          listProperties: ['from', 'to', 'type', 'weight'],
          showProperties: ['from', 'to', 'type', 'weight'],
          actions: {
            new: { isVisible: false },
            edit: { isVisible: false },
            delete: { isVisible: false },
          },
          sort: {
            sortBy: 'from',
            direction: 'asc' as const,
          },
        },
      },
      {
        resource: new SandboxMemoryResource(),
        options: {
          id: 'sandbox-memory',
          navigation: {
            name: 'Sandbox Memory',
            icon: 'Box',
          },
          listProperties: ['path', 'size', 'modifiedAt'],
          showProperties: ['path', 'size', 'modifiedAt', 'content'],
          actions: {
            new: { isVisible: false },
            edit: { isVisible: false },
            delete: { isVisible: false },
          },
          sort: {
            sortBy: 'modifiedAt',
            direction: 'desc' as const,
          },
        },
      },
    ],
    rootPath: '/admin',
    branding: {
      companyName: 'Tsuki Admin',
      withMadeWithLove: false,
      favicon: '/favicon.ico',
    },
    locale: {
      language: 'en',
      availableLanguages: ['en'],
      translations: {
        en: {
          labels: {
            threads: 'Threads',
            concepts: 'Concepts',
            episodes: 'Episodes',
            relations: 'Relations',
            'sandbox-memory': 'Sandbox Files',
          },
          properties: {
            id: 'ID',
            resourceId: 'Resource ID',
            title: 'Title',
            createdAt: 'Created Date',
            updatedAt: 'Updated Date',
            inputTokens: 'Input Tokens',
            outputTokens: 'Output Tokens',
            totalTokens: 'Total Tokens',
            reasoningTokens: 'Reasoning Tokens',
            cachedInputTokens: 'Cached Input Tokens',
            name: 'Name',
            summary: 'Summary',
            valence: 'Valence',
            arousalLevel: 'Arousal Level',
            accessedAt: 'Accessed Date',
            from: 'From',
            to: 'To',
            type: 'Type',
            weight: 'Weight',
            path: 'Path',
            content: 'Content',
            size: 'Size (bytes)',
            modifiedAt: 'Modified Date',
          },
          messages: {
            successfullyDeleted: 'Thread successfully deleted',
            thereWereValidationErrors: 'There were validation errors',
            forbiddenError: 'Forbidden: insufficient permissions',
          },
        },
      },
    },
  })

  return admin
}

export function createAdminRouter(
  config: ConfigService,
  agentMemory: MastraMemory,
): Router {
  const admin = createAdminJS(config, agentMemory)

  const authenticate = (email: string, password: string): { email: string; role: string } | null => {
    const expectedToken = process.env.WEB_AUTH_TOKEN
    if (typeof expectedToken !== 'string' || expectedToken.trim() === '') {
      logger.error('WEB_AUTH_TOKEN not configured')
      return null
    }

    if (password === expectedToken) {
      logger.info({ email }, 'AdminJS login successful')
      return { email, role: 'admin' }
    }

    logger.warn({ email }, 'AdminJS login failed')
    return null
  }

  const adminRouter = AdminJSExpress.buildAuthenticatedRouter(
    admin,
    {
      authenticate,
      cookieName: 'adminjs',
      cookiePassword: process.env.WEB_AUTH_TOKEN ?? 'default-secret',
    },
    null,
    {
      saveUninitialized: false,
      resave: false,
    },
  )

  const router = Router()
  router.use('/admin', adminRouter)

  return router
}
