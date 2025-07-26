import express from 'express'

export function authMiddleware(req: express.Request, res: express.Response, next: express.NextFunction): void | express.Response {
  const authHeader = req.headers.authorization

  if (typeof authHeader !== 'string' || authHeader.trim() === '') {
    return res.status(401).json({ error: 'Authorization header required' })
  }

  // Parse "username:token" format from Authorization header
  // Expected format: "username:token" (not "Bearer token")
  const credentials = authHeader
  const colonIndex = credentials.indexOf(':')

  if (colonIndex === -1) {
    return res.status(401).json({ error: 'Invalid authorization format. Expected "username:token"' })
  }

  const username = credentials.substring(0, colonIndex)
  const token = credentials.substring(colonIndex + 1)

  // Get expected token from environment
  const expectedToken = process.env.WEB_AUTH_TOKEN
  if (typeof expectedToken !== 'string' || expectedToken.trim() === '') {
    return res.status(500).json({ error: 'Server authentication not configured' })
  }

  // Verify token
  if (token !== expectedToken) {
    return res.status(401).json({ error: 'Invalid token' })
  }

  // Inject user into res.locals
  res.locals.user = username
  next()
}
