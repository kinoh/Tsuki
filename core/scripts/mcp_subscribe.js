// Use Mastra MCPClient.resources.subscribe to subscribe to messages

import { MCPClient } from '@mastra/mcp'

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

  client.resources.onUpdated('scheduler', (params) => {
    console.log('Received updated message from scheduler:', params)
  })

  client.resources.subscribe('scheduler', 'fired_schedule://recent').then(obj => {
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
    const response = await tools['scheduler_set_schedule'].execute({
      context: {
        name: 'test2',
        time: new Date().toISOString(),
        cycle: 'once',
        message: 'Hello hello',
      },
    })
    console.log(response)
  }
}

main().catch(console.error)
