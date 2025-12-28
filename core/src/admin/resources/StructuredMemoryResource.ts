import { BaseResource, BaseProperty, BaseRecord } from 'adminjs'
import { promises as fs } from 'fs'
import { join, extname } from 'path'
import { ConfigService } from '../../configService'
import { logger } from '../../logger'

interface StructuredMemoryDocument {
  id: string
  filename: string
  content: string
  size: number
  linkCount: number
  modifiedAt: Date
}

class StructuredMemoryProperty extends BaseProperty {
  constructor(
    private propertyName: string,
    private propertyType: 'string' | 'textarea' | 'datetime' | 'number' = 'string',
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
    return ['id', 'modifiedAt', 'size', 'linkCount'].includes(this.propertyName)
  }

  isId(): boolean {
    return this.propertyName === 'id'
  }
}

class StructuredMemoryRecord extends BaseRecord {
  constructor(private readonly document: StructuredMemoryDocument, resource: BaseResource) {
    super(document, resource)
  }
}

export class StructuredMemoryResource extends BaseResource {
  private dataDir: string

  constructor(private config: ConfigService) {
    super()
    this.dataDir = join(this.config.dataDir, `${process.env.PERMANENT_USERS}__structured_memory`)
  }

  id(): string {
    return 'structured-memory'
  }

  properties(): BaseProperty[] {
    return [
      new StructuredMemoryProperty('id', 'string'),
      new StructuredMemoryProperty('filename', 'string'),
      new StructuredMemoryProperty('size', 'number'),
      new StructuredMemoryProperty('linkCount', 'number'),
      new StructuredMemoryProperty('modifiedAt', 'datetime'),
      new StructuredMemoryProperty('content', 'textarea'),
    ]
  }

  property(path: string): BaseProperty | null {
    const properties = this.properties()
    return properties.find(prop => prop.path() === path) || null
  }

  private countLinks(content: string): number {
    const linkRegex = /\[\[([a-zA-Z0-9_-]+)\]\]/g
    const matches = content.match(linkRegex)
    return matches ? matches.length : 0
  }

  private async readDocuments(): Promise<StructuredMemoryDocument[]> {
    try {
      await fs.access(this.dataDir)
    } catch {
      // Directory doesn't exist
      return []
    }

    try {
      const files = await fs.readdir(this.dataDir)
      const markdownFiles = files.filter(file => extname(file) === '.md')
      
      const documents: StructuredMemoryDocument[] = []
      
      for (const filename of markdownFiles) {
        const filePath = join(this.dataDir, filename)
        try {
          const [content, stats] = await Promise.all([
            fs.readFile(filePath, 'utf-8'),
            fs.stat(filePath),
          ])
          
          const id = filename.replace('.md', '')
          const linkCount = this.countLinks(content)
          
          documents.push({
            id,
            filename,
            content,
            size: stats.size,
            linkCount,
            modifiedAt: stats.mtime,
          })
        } catch (err) {
          logger.error({ err, filename }, 'Error reading file')
        }
      }
      
      return documents
    } catch (err) {
      logger.error({ err }, 'Error reading structured memory directory')
      return []
    }
  }

  async count(): Promise<number> {
    try {
      const documents = await this.readDocuments()
      return documents.length
    } catch (err) {
      logger.error({ err }, 'Error counting structured memory documents')
      return 0
    }
  }

  async find(_filters: unknown, options: unknown): Promise<BaseRecord[]> {
    try {
      const optionsObj = options as { limit?: number; offset?: number; sort?: { sortBy?: string; direction?: 'asc' | 'desc' } } | undefined
      const limit = optionsObj?.limit ?? 10
      const offset = optionsObj?.offset ?? 0
      const sortBy = optionsObj?.sort?.sortBy ?? 'modifiedAt'
      const direction = optionsObj?.sort?.direction ?? 'desc'

      let documents = await this.readDocuments()
      
      // Sort documents
      documents.sort((a, b) => {
        let aValue: string | number | Date
        let bValue: string | number | Date
        
        switch (sortBy) {
          case 'size':
          case 'linkCount':
            aValue = a[sortBy]
            bValue = b[sortBy]
            break
          case 'modifiedAt':
            aValue = a.modifiedAt
            bValue = b.modifiedAt
            break
          default:
            aValue = a.id
            bValue = b.id
        }
        
        if (direction === 'desc') {
          return aValue > bValue ? -1 : aValue < bValue ? 1 : 0
        } else {
          return aValue < bValue ? -1 : aValue > bValue ? 1 : 0
        }
      })
      
      // Apply pagination
      documents = documents.slice(offset, offset + limit)
      
      return documents.map(doc => new StructuredMemoryRecord(doc, this))
    } catch (err) {
      logger.error({ err }, 'Error finding structured memory documents')
      return []
    }
  }

  async findOne(id: string): Promise<BaseRecord | null> {
    try {
      const documents = await this.readDocuments()
      const document = documents.find(doc => doc.id === id)
      
      if (!document) {
        return null
      }
      
      return new StructuredMemoryRecord(document, this)
    } catch (err) {
      logger.error({ err, documentId: id }, 'Error finding structured memory document')
      return null
    }
  }

  create(): Promise<BaseRecord> {
    throw new Error('Document creation not allowed via admin panel')
  }

  update(): Promise<BaseRecord> {
    throw new Error('Document update not allowed via admin panel')
  }

  delete(): Promise<void> {
    throw new Error('Document deletion not allowed via admin panel')
  }
}
