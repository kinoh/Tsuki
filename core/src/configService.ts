import { mkdirSync, readdirSync } from 'fs'
import { spawnSync } from 'child_process'

export class ConfigService {
  public readonly dataDir: string
  public readonly env: string
  public readonly isProduction: boolean

  constructor() {
    this.env = process.env.ENV ?? process.env.NODE_ENV ?? 'development'
    this.isProduction = this.env === 'production'
    this.dataDir = process.env.DATA_DIR ?? './data'

    this.initDataDir()
  }

  private initDataDir(): void {
    mkdirSync(this.dataDir, { recursive: true })

    console.log(`Data directory: ${this.dataDir}`)

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
    console.log(`Restoring data from file: ${backupFile}`)

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
}
