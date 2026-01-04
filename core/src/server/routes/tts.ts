import { Request, Response } from 'express'
import fetch, { type RequestInit, type Response as FetchResponse } from 'node-fetch'
import { logger } from '../../logger'

type TTSRequest = {
  message: string,
}

type AudioQuery = Record<string, unknown>

const DEFAULT_VOICEVOX_ENDPOINT = 'http://voicevox-engine:50021'
const DEFAULT_VOICEVOX_SPEAKER = 1
const DEFAULT_VOICEVOX_TIMEOUT_MS = 10000

function isTTSRequest(payload: unknown): payload is TTSRequest {
  if (payload === null || typeof payload !== 'object') {
    return false
  }

  const candidate = payload as Record<string, unknown>
  return typeof candidate.message === 'string'
}

function getVoicevoxEndpoint(): string {
  const raw = process.env.VOICEVOX_ENDPOINT ?? DEFAULT_VOICEVOX_ENDPOINT
  return raw.replace(/\/$/, '')
}

function getVoicevoxSpeaker(): number {
  const raw = process.env.VOICEVOX_SPEAKER ?? DEFAULT_VOICEVOX_SPEAKER.toString()
  const parsed = Number.parseInt(raw, 10)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_VOICEVOX_SPEAKER
}

function getVoicevoxTimeoutMs(): number {
  const raw = process.env.VOICEVOX_TIMEOUT_MS ?? DEFAULT_VOICEVOX_TIMEOUT_MS.toString()
  const parsed = Number.parseInt(raw, 10)
  return Number.isFinite(parsed) && parsed > 0 ? parsed : DEFAULT_VOICEVOX_TIMEOUT_MS
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

  const endpoint = getVoicevoxEndpoint()
  const speaker = getVoicevoxSpeaker()
  const timeoutMs = getVoicevoxTimeoutMs()

  try {
    const queryUrl = new URL('/audio_query', endpoint)
    queryUrl.searchParams.set('speaker', speaker.toString())
    queryUrl.searchParams.set('text', message)

    const queryResponse = await fetchWithTimeout(queryUrl.toString(), { method: 'POST' }, timeoutMs)
    if (!queryResponse.ok) {
      const body = await safeReadText(queryResponse)
      logger.error({ status: queryResponse.status, body }, 'VoiceVox audio_query failed')
      res.status(502).json({ error: 'VoiceVox audio query failed' })
      return
    }

    const query = await queryResponse.json() as AudioQuery

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
