import AdminJS from 'adminjs'
import { Router } from 'express'
import * as AdminJSExpress from '@adminjs/express'
import { MastraMemory } from '@mastra/core'
import { ThreadResource } from './resources/ThreadResource.js'

export function createAdminJS(agentMemory: MastraMemory): AdminJS {
  const admin = new AdminJS({
    resources: [{
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
            isVisible: false, // Disable thread creation
          },
          edit: {
            isVisible: false, // Disable thread editing
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
        },
        sort: {
          sortBy: 'id',
          direction: 'desc' as const,
        },
      },
    }],
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
          },
          properties: {
            id: 'Thread ID',
            resourceId: 'Resource ID',
            title: 'Title',
            createdAt: 'Created Date',
            updatedAt: 'Updated Date',
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
