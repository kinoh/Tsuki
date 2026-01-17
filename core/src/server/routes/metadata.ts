import express from 'express'
import { logger } from '../../internal/logger'

async function getGitHash(): Promise<string | null> {
  // In Docker environment, get from environment variable
  if (process.env.GIT_HASH !== undefined) {
    return process.env.GIT_HASH
  }

  // In development environment, get from git command
  try {
    const { execSync } = await import('child_process')
    const hash = execSync('git rev-parse HEAD', { encoding: 'utf8' }).trim()
    return hash
  } catch (err) {
    logger.warn({ err }, 'Failed to get git hash')
    return null
  }
}

export async function metadataHandler(req: express.Request, res: express.Response): Promise<void> {
  try {
    const agent = req.app.locals.agent
    const gitHash = await getGitHash()
    const openaiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'
    const tools = await agent.listTools()
    const mcpTools = Object.keys(tools)

    res.json({
      git_hash: gitHash,
      openai_model: openaiModel,
      mcp_tools: mcpTools,
    })
  } catch (err) {
    logger.error({ err }, 'Error fetching metadata')
    res.status(500).json({ error: 'Internal server error' })
  }
}
