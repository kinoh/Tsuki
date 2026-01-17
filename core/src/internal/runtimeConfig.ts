import { readFile, writeFile } from 'fs/promises'
import { join } from 'path'
import { logger } from './logger'
import type { components } from '../shared/openapi'

export type RuntimeConfig = components['schemas']['Config']

type RuntimeConfigListener = (config: RuntimeConfig) => void

const DEFAULT_CONFIG: RuntimeConfig = {
  enableNotification: true,
  enableSensory: true,
}

export class RuntimeConfigStore {
  private config: RuntimeConfig = { ...DEFAULT_CONFIG }
  private listeners = new Set<RuntimeConfigListener>()
  private readonly filePath: string

  constructor(dataDir: string) {
    this.filePath = join(dataDir, 'config.json')
  }

  get(): RuntimeConfig {
    return { ...this.config }
  }

  async load(): Promise<RuntimeConfig> {
    try {
      const raw = await readFile(this.filePath, 'utf-8')
      const parsed = JSON.parse(raw) as unknown
      const normalized = parseRuntimeConfig(parsed)
      this.config = normalized
      return this.get()
    } catch (err) {
      if (isNotFoundError(err)) {
        await this.persist(this.config)
        return this.get()
      }

      logger.error({ err }, 'Failed to load runtime config, restoring defaults')
      this.config = { ...DEFAULT_CONFIG }
      await this.persist(this.config)
      return this.get()
    }
  }

  async set(next: RuntimeConfig): Promise<RuntimeConfig> {
    const normalized = parseRuntimeConfig(next)
    const changed = !isSameConfig(this.config, normalized)
    this.config = normalized
    await this.persist(this.config)

    if (changed) {
      this.notify()
    }

    return this.get()
  }

  onChange(listener: RuntimeConfigListener): () => void {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  private notify(): void {
    for (const listener of this.listeners) {
      listener(this.get())
    }
  }

  private async persist(config: RuntimeConfig): Promise<void> {
    const payload = JSON.stringify(config, null, 2)
    await writeFile(this.filePath, payload, 'utf-8')
  }
}

function parseRuntimeConfig(input: unknown): RuntimeConfig {
  if (!isRuntimeConfig(input)) {
    throw new Error('Invalid runtime config payload')
  }

  return {
    enableNotification: input.enableNotification,
    enableSensory: input.enableSensory,
  }
}

function isRuntimeConfig(input: unknown): input is RuntimeConfig {
  if (input === null || typeof input !== 'object') {
    return false
  }

  const candidate = input as Record<string, unknown>
  return typeof candidate.enableNotification === 'boolean' && typeof candidate.enableSensory === 'boolean'
}

function isSameConfig(a: RuntimeConfig, b: RuntimeConfig): boolean {
  return a.enableNotification === b.enableNotification && a.enableSensory === b.enableSensory
}

function isNotFoundError(error: unknown): boolean {
  return typeof error === 'object' && error !== null && 'code' in error && (error as { code?: string }).code === 'ENOENT'
}
