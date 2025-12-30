import fs from 'fs/promises'

type LogLine = {
  event?: string
  time?: number
  payload?: {
    type?: string
    text?: string
  }
  message?: {
    chat?: string[]
  }
}

async function run(): Promise<void> {
  const filePath = process.argv[2]
  if (!filePath) {
    console.error('Usage: pnpm tsx ./tests/client/format_log.ts <log.jsonl>')
    process.exit(1)
  }

  const content = await fs.readFile(filePath, 'utf-8')
  const lines = content.split('\n').filter((line) => line.trim().length > 0)

  let lastSendTime: number | null = null

  for (const line of lines) {
    const item = JSON.parse(line) as LogLine

    if (item.event === 'send') {
      const payloadType = item.payload?.type
      const label = payloadType && payloadType !== 'message' ? payloadType : 'user'
      const text = item.payload?.text ?? ''
      console.log(`${label}: ${text}`)
      if (typeof item.time === 'number') {
        lastSendTime = item.time
      }
      continue
    }

    if (item.event === 'receive') {
      const chats = Array.isArray(item.message?.chat) ? item.message?.chat ?? [] : []
      if (chats.length === 0) continue
      const latencyMs =
        typeof item.time === 'number' && typeof lastSendTime === 'number'
          ? item.time - lastSendTime
          : undefined

      chats.forEach((chat, index) => {
        const isLast = index === chats.length - 1
        if (isLast && typeof latencyMs === 'number') {
          console.log(`agent: ${chat} (${latencyMs}ms)`)
        } else {
          console.log(`agent: ${chat}`)
        }
      })
    }
  }
}

run().catch((error) => {
  console.error(error)
  process.exit(1)
})
