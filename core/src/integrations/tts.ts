import fetch, { type RequestInit, type Response as FetchResponse } from 'node-fetch'
import { ConfigService } from '../internal/configService'
import { logger } from '../internal/logger'

type Accent = Record<string, unknown>
type AudioQuery = Record<string, unknown>

const DEFAULT_VOICEVOX_SPEAKER = 10
const DEFAULT_VOICEVOX_TIMEOUT_MS = 10000

export type TtsSynthesisResult = {
  audioBuffer: Buffer
  contentLength: string | null
}

export class VoiceVoxError extends Error {
  public readonly stage: 'accent_phrases' | 'synthesis'
  public readonly status: number
  public readonly body: string | null

  constructor(stage: 'accent_phrases' | 'synthesis', status: number, body: string | null) {
    super(`VoiceVox ${stage} failed`)
    this.stage = stage
    this.status = status
    this.body = body
  }
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

export async function synthesizeTts(message: string, config: ConfigService): Promise<TtsSynthesisResult> {
  const speaker = getVoicevoxSpeaker()
  const timeoutMs = getVoicevoxTimeoutMs()

  const accentUrl = new URL('/accent', config.jaAccentUrl)
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

  logger.info({ message, accent: accent.accent }, 'Generated accent from ja-accent')

  const phrasesUrl = new URL('/accent_phrases', config.voicevoxUrl)
  phrasesUrl.searchParams.set('speaker', speaker.toString())
  phrasesUrl.searchParams.set('text', accent.accent as string)
  phrasesUrl.searchParams.set('is_kana', 'true')

  const phrasesResponse = await fetchWithTimeout(phrasesUrl.toString(), { method: 'POST' }, timeoutMs)
  if (!phrasesResponse.ok) {
    const body = await safeReadText(phrasesResponse)
    logger.error({ status: phrasesResponse.status, body }, 'VoiceVox accent_phrases failed')
    throw new VoiceVoxError('accent_phrases', phrasesResponse.status, body)
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

  const synthUrl = new URL('/synthesis', config.voicevoxUrl)
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
    throw new VoiceVoxError('synthesis', synthResponse.status, body)
  }

  const audioBuffer = Buffer.from(await synthResponse.arrayBuffer())
  const contentLength = synthResponse.headers.get('content-length')

  return {
    audioBuffer,
    contentLength,
  }
}
