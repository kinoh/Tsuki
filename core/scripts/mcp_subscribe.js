// Use Mastra MCPClient.resources.subscribe to subscribe to messages

import { MCPClient } from '@mastra/mcp'
import fs from 'fs'

async function main() {
  const client = new MCPClient({
    servers: {
      scheduler: {
        command: './bin/scheduler',
        args: [],
        env: {
          DATA_DIR: '/tmp/tsuki_test_scheduler',
          SCHEDULER_LOOP_INTERVAL_MS: '100',
          TZ: 'Asia/Tokyo',
        },
      },
    },
  })

  const tools = await client.getTools()

  await client.resources.onUpdated('scheduler', (params) => {
    console.log('Received updated message from scheduler:', params)
  })

  await client.resources.subscribe('scheduler', 'fired_schedule://recent').then(obj => {
    console.log('subscription started')
  })

  {
    const response = await tools['scheduler_set_schedule'].execute({
      context: {
        name: 'test1',
        time: new Date().toISOString(),
        cycle: 'once',
        message: 'Hello, World!',
      },
    })
    console.log(response)
  }
  {
    const now = new Date()
    const response = await tools['scheduler_set_schedule'].execute({
      context: {
        name: 'test2',
        // HH:MM:SS expected
        time: `${String(now.getHours()).padStart(2, '0')}:${String(now.getMinutes()).padStart(2, '0')}:${String(now.getSeconds()).padStart(2, '0')}`,
        cycle: 'daily',
        message: 'Hello hello',
      },
    })
    console.log(response)
  }

  setTimeout(() => {

    console.log('Exiting...')
    client.disconnect()

    // Remove data_dir
    fs.rm('/tmp/tsuki_test_scheduler', { recursive: true, force: true }, (err) => {
      if (err) {
        console.error('Error removing data_dir:', err)
      } else {
        console.log('data_dir removed')
      }
    })
  }, 3000);
}

main().catch(console.error)
