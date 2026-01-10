import { Request, Response } from 'express'
import fetch, { type RequestInit, type Response as FetchResponse } from 'node-fetch'
import { logger } from '../../logger'
import { ConfigService } from '../../configService'

type TTSRequest = {
  message: string,
}

type Accent = Record<string, unknown>
type AudioQuery = Record<string, unknown>

const DEFAULT_VOICEVOX_SPEAKER = 10
const DEFAULT_VOICEVOX_TIMEOUT_MS = 10000

function getConfig(req: Request): ConfigService {
  return req.app.locals.config as ConfigService
}

function getJaAccentEndpoint(isProduction: boolean): string {
  const raw = process.env.JA_ACCENT_ENDPOINT
  const endpoint = typeof raw === 'string' && raw.trim().length > 0
    ? raw.trim()
    : (isProduction ? 'http://ja-accent:2954' : 'http://localhost:2954')
  return endpoint.replace(/\/$/, '')
}

function getVoicevoxEndpoint(isProduction: boolean): string {
  const raw = process.env.VOICEVOX_ENDPOINT
  const endpoint = typeof raw === 'string' && raw.trim().length > 0
    ? raw.trim()
    : (isProduction ? 'http://voicevox-engine:50021' : 'http://localhost:50021')
  return endpoint.replace(/\/$/, '')
}

function parsePositiveInt(value: string | undefined, fallback: number): number {
  if (value === undefined || value.trim() === '') {
    return fallback
  }
  const parsed = Number.parseInt(value, 10)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : fallback
}

function getVoicevoxSpeaker(): number {
  return parsePositiveInt(process.env.VOICEVOX_SPEAKER, DEFAULT_VOICEVOX_SPEAKER)
}

function getVoicevoxTimeoutMs(): number {
  return parsePositiveInt(process.env.VOICEVOX_TIMEOUT_MS, DEFAULT_VOICEVOX_TIMEOUT_MS)
}

function isTTSRequest(payload: unknown): payload is TTSRequest {
  if (payload === null || typeof payload !== 'object') {
    return false
  }

  const candidate = payload as Record<string, unknown>
  return typeof candidate.message === 'string'
}

async function fetchWithTimeout(url: string, init: RequestInit, timeoutMs: number): Promise<FetchResponse> {
  const controller = new AbortController()
  const timeout = setTimeout(() => controller.abort(), timeoutMs)

  try {
    return await fetch(url, { ...init, signal: controller.signal })
  } finally {
    clearTimeout(timeout)
  }
}

async function safeReadText(response: FetchResponse): Promise<string | null> {
  try {
    return await response.text()
  } catch {
    return null
  }
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
  const endpoint = getVoicevoxEndpoint(config.isProduction)
  const speaker = getVoicevoxSpeaker()
  const timeoutMs = getVoicevoxTimeoutMs()

  try {
    const accentUrl = new URL('/accent', getJaAccentEndpoint(config.isProduction))
    const accentResponse = await fetchWithTimeout(
      accentUrl.toString(),
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ 'text': message }),
      },
      timeoutMs,
    )
    const accent = await accentResponse.json() as Accent

    const phrasesUrl = new URL('/accent_phrases', endpoint)
    phrasesUrl.searchParams.set('speaker', speaker.toString())
    phrasesUrl.searchParams.set('text', accent.accent as string)
    phrasesUrl.searchParams.set('is_kana', 'true')

    const phrasesResponse = await fetchWithTimeout(phrasesUrl.toString(), { method: 'POST' }, timeoutMs)
    if (!phrasesResponse.ok) {
      const body = await safeReadText(phrasesResponse)
      logger.error({ status: phrasesResponse.status, body }, 'VoiceVox accent_phrases failed')
      res.status(502).json({ error: 'VoiceVox accent_phrases failed' })
      return
    }

    const query = {
      'accent_phrases': await phrasesResponse.json(),
    } as AudioQuery

    query.speedScale = 1.15
    query.pitchScale = -0.02
    query.intonationScale = 1.4
    query.volumeScale = 1.0
    query.pauseLengthScale = 0.4
    query.prePhonemeLength = 0
    query.postPhonemeLength = 0
    query.outputSamplingRate = 24000
    query.outputStereo = false

    const synthUrl = new URL('/synthesis', endpoint)
    synthUrl.searchParams.set('speaker', speaker.toString())

    const synthResponse = await fetchWithTimeout(
      synthUrl.toString(),
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(query),
      },
      timeoutMs,
    )

    if (!synthResponse.ok) {
      const body = await safeReadText(synthResponse)
      logger.error({ status: synthResponse.status, body }, 'VoiceVox synthesis failed')
      res.status(502).json({ error: 'VoiceVox synthesis failed' })
      return
    }

    const audioBuffer = Buffer.from(await synthResponse.arrayBuffer())
    const contentLength = synthResponse.headers.get('content-length')

    res.status(200)
    res.setHeader('Content-Type', 'audio/wav')
    if (typeof contentLength === 'string' && contentLength.length > 0) {
      res.setHeader('Content-Length', contentLength)
    }
    res.send(audioBuffer)
  } catch (err) {
    if (err instanceof Error && err.name === 'AbortError') {
      logger.error({ err }, 'VoiceVox request timed out')
      res.status(504).json({ error: 'VoiceVox request timed out' })
      return
    }

    logger.error({ err }, 'Unexpected error in TTS handler')
    res.status(500).json({ error: 'Internal server error' })
  }
}
