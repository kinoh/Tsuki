import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import { promises as fs } from 'fs'
import path from 'path'
import { logger } from '../../internal/logger'

interface SandboxFileEntry {
  path: string
  size: number
  modifiedAt: Date
}

interface SandboxFileRecord extends SandboxFileEntry {
  content: string
}

const SANDBOX_ROOT = '/memory'
const MAX_CONTENT_BYTES = 128 * 1024

class SandboxMemoryProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'datetime' | 'number' | 'textarea' = 'string',
  ) {
    super({ path: propertyName, type: propertyType })
  }

  name(): string {
    return this.propertyName
  }

  path(): string {
    return this.propertyName
  }

  isEditable(): boolean {
    return false
  }

  isVisible(): boolean {
    return true
  }

  isSortable(): boolean {
    return ['path', 'size', 'modifiedAt'].includes(this.propertyName)
  }

  isId(): boolean {
    return this.propertyName === 'path'
  }
}

class SandboxMemoryRecord extends BaseRecord {
  constructor(private readonly entry: SandboxFileRecord, resource: BaseResource) {
    super(entry, resource)
  }

  id(): string {
    return encodeRecordId(this.entry.path)
  }
}

function toRelativePath(root: string, filePath: string): string {
  const relative = path.relative(root, filePath)
  return relative.split(path.sep).join('/')
}

function encodeRecordId(relativePath: string): string {
  return relativePath.replace(/\|/g, '||').replace(/\//g, '|')
}

function decodeRecordId(id: string): string | null {
  if (!id.includes('|')) {
    return id
  }
  const placeholder = '\u0000'
  const protectedValue = id.replace(/\|\|/g, placeholder)
  if (protectedValue.includes('|')) {
    const restored = protectedValue.replace(/\|/g, '/')
    return restored.replace(new RegExp(placeholder, 'g'), '|')
  }
  return protectedValue.replace(new RegExp(placeholder, 'g'), '|')
}

function resolveSandboxPath(root: string, relativePath: string): string | null {
  const normalized = relativePath.replace(/\\/g, '/')
  const resolved = path.resolve(root, normalized)
  const safeRoot = root.endsWith(path.sep) ? root : `${root}${path.sep}`
  if (!resolved.startsWith(safeRoot)) {
    return null
  }
  return resolved
}

export class SandboxMemoryResource extends BaseResource {
  private readonly root: string

  constructor() {
    super()
    this.root = path.resolve(SANDBOX_ROOT)
  }

  id(): string {
    return 'sandbox-memory'
  }

  properties(): BaseProperty[] {
    return [
      new SandboxMemoryProperty('path', 'string'),
      new SandboxMemoryProperty('size', 'number'),
      new SandboxMemoryProperty('modifiedAt', 'datetime'),
      new SandboxMemoryProperty('content', 'textarea'),
    ]
  }

  property(pathName: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === pathName) || null
  }

  private async listFiles(dir: string): Promise<SandboxFileEntry[]> {
    try {
      const entries = await fs.readdir(dir, { withFileTypes: true })
      const results: SandboxFileEntry[] = []

      for (const entry of entries) {
        const entryPath = path.join(dir, entry.name)
        if (entry.isDirectory()) {
          const nested = await this.listFiles(entryPath)
          results.push(...nested)
        } else if (entry.isFile()) {
          const stats = await fs.stat(entryPath)
          results.push({
            path: toRelativePath(this.root, entryPath),
            size: stats.size,
            modifiedAt: stats.mtime,
          })
        }
      }

      return results
    } catch (err) {
      if ((err as NodeJS.ErrnoException).code === 'ENOENT') {
        return []
      }
      logger.error({ err, dir }, 'Error reading sandbox memory directory')
      return []
    }
  }

  private async readContent(relativePath: string): Promise<string> {
    const resolved = resolveSandboxPath(this.root, relativePath)
    if (resolved === null) {
      return ''
    }

    try {
      const buffer = await fs.readFile(resolved)
      const truncated = buffer.length > MAX_CONTENT_BYTES
      const text = buffer.toString('utf-8').replace(/\r\n/g, '\n')
      const content = truncated ? text.slice(0, MAX_CONTENT_BYTES) : text
      const warning = truncated
        ? `WARNING: truncated to ${MAX_CONTENT_BYTES} bytes\n`
        : ''
      return `${warning}${content}`.trimEnd()
    } catch (err) {
      logger.error({ err, relativePath }, 'Error reading sandbox memory file')
      return ''
    }
  }

  async count(): Promise<number> {
    const files = await this.listFiles(this.root)
    return files.length
  }

  async find(_filters: unknown, options: unknown): Promise<BaseRecord[]> {
    const optionsObj = options as { limit?: number; offset?: number; sort?: { sortBy?: string; direction?: 'asc' | 'desc' } } | undefined
    const limit = optionsObj?.limit ?? 10
    const offset = optionsObj?.offset ?? 0
    const sortBy = optionsObj?.sort?.sortBy ?? 'modifiedAt'
    const direction = optionsObj?.sort?.direction ?? 'desc'

    let files = await this.listFiles(this.root)
    files.sort((a, b) => {
      let aValue: string | number | Date
      let bValue: string | number | Date

      switch (sortBy) {
        case 'size':
          aValue = a.size
          bValue = b.size
          break
        case 'path':
          aValue = a.path
          bValue = b.path
          break
        default:
          aValue = a.modifiedAt
          bValue = b.modifiedAt
      }

      if (direction === 'asc') {
        return aValue < bValue ? -1 : aValue > bValue ? 1 : 0
      }
      return aValue > bValue ? -1 : aValue < bValue ? 1 : 0
    })

    files = files.slice(offset, offset + limit)
    const records = files.map(file => ({
      ...file,
      content: '',
    }))
    return records.map(record => new SandboxMemoryRecord(record, this))
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    const decodedId = decodeRecordId(id)
    if (decodedId === null || decodedId === '') {
      return null
    }
    const resolved = resolveSandboxPath(this.root, decodedId)
    if (resolved === null) {
      return null
    }

    try {
      const stats = await fs.stat(resolved)
      if (!stats.isFile()) {
        return null
      }
      const content = await this.readContent(decodedId)
      const entry: SandboxFileRecord = {
        path: decodedId,
        size: stats.size,
        modifiedAt: stats.mtime,
        content,
      }
      return new SandboxMemoryRecord(entry, this)
    } catch (err) {
      logger.error({ err, id }, 'Error loading sandbox memory file')
      return null
    }
  }

  create(): Promise<BaseRecord> {
    throw new Error('Sandbox memory creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Sandbox memory update not allowed via admin panel')
  }

  delete(): Promise<void> {
    throw new Error('Sandbox memory deletion not allowed via admin panel')
  }
}
