import { Request, Response } from 'express'
import { ConfigService } from '../../internal/configService'
import { synthesizeTts, VoiceVoxError } from '../../integrations/tts'
import { logger } from '../../internal/logger'

type TTSRequest = {
  message: string,
}

function getConfig(req: Request): ConfigService {
  return req.app.locals.config as ConfigService
}

function isTTSRequest(payload: unknown): payload is TTSRequest {
  if (payload === null || typeof payload !== 'object') {
    return false
  }

  const candidate = payload as Record<string, unknown>
  return typeof candidate.message === 'string'
}

export async function ttsHandler(req: Request, res: Response): Promise<void> {
  if (!isTTSRequest(req.body)) {
    res.status(400).json({ error: 'Invalid payload' })
    return
  }

  const message = req.body.message.trim()
  if (message.length === 0) {
    res.status(400).json({ error: 'Message is required' })
    return
  }

  const config = getConfig(req)
  try {
    const { audioBuffer, contentLength } = await synthesizeTts(message, config)

    res.status(200)
    res.setHeader('Content-Type', 'audio/wav')
    if (typeof contentLength === 'string' && contentLength.length > 0) {
      res.setHeader('Content-Length', contentLength)
    }
    res.send(audioBuffer)
  } catch (err) {
    if (err instanceof VoiceVoxError) {
      const errorMessage = err.stage === 'accent_phrases'
        ? 'VoiceVox accent_phrases failed'
        : 'VoiceVox synthesis failed'
      res.status(502).json({ error: errorMessage })
      return
    }

    if (err instanceof Error && err.name === 'AbortError') {
      logger.error({ err }, 'TTS request timed out')
      res.status(504).json({ error: 'TTS request timed out' })
      return
    }

    logger.error({ err }, 'Unexpected error in TTS handler')
    res.status(500).json({ error: 'Internal server error' })
  }
}
