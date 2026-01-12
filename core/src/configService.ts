import { mkdirSync, readdirSync } from 'fs'
import { spawnSync } from 'child_process'
import { logger } from './logger'

export class ConfigService {
  public readonly dataDir: string
  public readonly env: string
  public readonly isProduction: boolean
  public readonly serverPort: number
  public readonly memgraphUri: string
  public readonly sandboxMcpUrl: string
  public readonly traceTools: boolean
  public readonly timeZone: string

  constructor() {
    this.env = process.env.ENV ?? process.env.NODE_ENV ?? 'development'
    this.isProduction = this.env === 'production'
    this.memgraphUri = process.env.MEMGRAPH_URI ?? 'bolt://memgraph:7687'
    this.sandboxMcpUrl = process.env.SANDBOX_MCP_URL ?? 'http://sandbox:8000/mcp'
    this.dataDir = process.env.DATA_DIR ?? './data'
    this.serverPort = Number(process.env.PORT ?? 2953)
    this.traceTools = ConfigService.parseBooleanFlag(process.env.TRACE_TOOLS)
    this.timeZone = process.env.TZ ?? 'Asia/Tokyo'

    this.initDataDir()
  }

  private initDataDir(): void {
    mkdirSync(this.dataDir, { recursive: true })

    logger.info({ dataDir: this.dataDir }, 'ConfigService: Data directory initialized')

    // If directory is empty
    if (readdirSync(this.dataDir).length === 0) {
      const backupFile = process.env.DEBUG_INIT_DATA_BACKUP
      if (backupFile !== undefined) {
        this.loadBackup(backupFile)
      }
    }
  }

  /**
   * Load data from a tarball file (typically created by `task backup` command)
   * @param backupFile tarball file path
   */
  private loadBackup(backupFile: string): void {
    logger.info('Restoring data from file')

    // Allow external command because this is only for development environment
    const result = spawnSync('tar', ['-xzf', backupFile, '-C', this.dataDir], {
      stdio: 'inherit',
    })
    if (result.status !== 0) {
      const message =
        result.error?.message ??
        `tar exited with status ${result.status ?? 'unknown'}`
      throw new Error(`Failed to restore data: ${message}`)
    }
  }

  private static parseBooleanFlag(value: string | undefined): boolean {
    if (value === undefined) {
      return false
    }
    const normalized = value.trim().toLowerCase()
    return normalized === '1' || normalized === 'true' || normalized === 'yes' || normalized === 'on'
  }
}
