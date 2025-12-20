export class ConfigService {
  get dataDir(): string {
    return process.env.DATA_DIR ?? './data'
  }
}
