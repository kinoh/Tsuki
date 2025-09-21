import { MastraStorage } from '@mastra/core/storage'

export interface LibSQLClient {
  execute: (params: string | { sql: string; args: (string | number)[] }) => Promise<{
    rows: Array<Record<string, string | number>>
  }>
}

export function getClient(storage: MastraStorage): LibSQLClient {
  // Access private LibSQL client directly
  // eslint-disable-next-line @typescript-eslint/no-explicit-any, @typescript-eslint/no-unsafe-member-access
  return (storage as any).client as LibSQLClient
}
