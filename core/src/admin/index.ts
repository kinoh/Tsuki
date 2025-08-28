import AdminJS, { ActionContext, ActionResponse, BaseRecord } from 'adminjs'
import { Router } from 'express'
import * as AdminJSExpress from '@adminjs/express'
import { MastraMemory } from '@mastra/core'
import { ThreadResource } from './resources/ThreadResource'
import { MessageResource } from './resources/MessageResource'
import { StructuredMemoryResource } from './resources/StructuredMemoryResource'

export function createAdminJS(agentMemory: MastraMemory): AdminJS {
  const admin = new AdminJS({
    resources: [
      {
        resource: new ThreadResource(agentMemory),
        options: {
          id: 'threads',
          navigation: {
            name: 'Thread Management',
            icon: 'MessageSquare',
          },
          listProperties: ['id', 'resourceId', 'title', 'createdAt', 'updatedAt'],
          showProperties: ['id', 'resourceId', 'title', 'createdAt', 'updatedAt'],
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
          navigation: null, // Do not show in navigation
          listProperties: ['id', 'role', 'user', 'chat', 'timestamp'],
          showProperties: ['id', 'role', 'user', 'chat', 'timestamp'],
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
        resource: new StructuredMemoryResource(),
        options: {
          id: 'structured-memory',
          navigation: {
            name: 'Structured Memory',
            icon: 'FileText',
          },
          listProperties: ['id', 'filename', 'size', 'linkCount', 'modifiedAt'],
          showProperties: ['id', 'filename', 'content', 'size', 'linkCount', 'modifiedAt'],
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
            'structured-memory': 'Documents',
          },
          properties: {
            id: 'ID',
            resourceId: 'Resource ID',
            title: 'Title',
            createdAt: 'Created Date',
            updatedAt: 'Updated Date',
            filename: 'Filename',
            content: 'Content',
            size: 'Size (bytes)',
            linkCount: 'Link Count',
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
  agentMemory: MastraMemory,
): Router {
  const admin = createAdminJS(agentMemory)

  const authenticate = (email: string, password: string): { email: string; role: string } | null => {
    const expectedToken = process.env.WEB_AUTH_TOKEN
    if (typeof expectedToken !== 'string' || expectedToken.trim() === '') {
      console.error('WEB_AUTH_TOKEN not configured')
      return null
    }

    if (password === expectedToken) {
      console.log(`AdminJS login successful for: ${email}`)
      return { email, role: 'admin' }
    }

    console.log(`AdminJS login failed for: ${email}`)
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
