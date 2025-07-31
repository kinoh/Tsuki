#!/usr/bin/env node

import { readFile } from 'fs/promises'
import { resolve } from 'path'
import { fileURLToPath } from 'url'
import { dirname } from 'path'
import { parse as parseYaml } from 'yaml'
import dotenv from 'dotenv'

// Load environment variables
dotenv.config()

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

async function loadAgent() {
  // Import the tsuki agent
  const { tsuki } = await import('../src/mastra/agents/tsuki.ts')
  return tsuki
}

async function loadInstructions() {
  // Import and use loadPromptFromEnv like the main application
  const { loadPromptFromEnv } = await import('../src/prompt.ts')
  return await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
}

async function loadTestInputs() {
  const testInputsPath = resolve(__dirname, './test_agent.yaml')
  const data = await readFile(testInputsPath, 'utf-8')
  const yamlData = parseYaml(data)

  return yamlData.inputs
}

async function runTests() {
  console.log('🤖 Loading Tsuki agent and instructions...')

  try {
    const agent = await loadAgent()
    const instructions = await loadInstructions()
    const testInputs = await loadTestInputs()

    console.log(`prompt: ${instructions}\n`)

    console.log(`📝 Running ${testInputs.length} test cases...\n`)
    console.log(`📋 Using encrypted prompt system like main application`)

    for (let i = 0; i < testInputs.length; i++) {
      const input = testInputs[i]
      console.log(`\n${'='.repeat(60)}`)
      console.log(`🧪 Test ${i + 1}/${testInputs.length}`)
      console.log(`💬 Input: "${input}"`)

      try {
        const startTime = Date.now()

        // Create runtime context for the agent (same as main application)
        const runtimeContext = new Map()
        runtimeContext.set('instructions', instructions)

        const result = await agent.generate([{
          role: 'user',
          content: input
        }], {
          runtimeContext
        })

        const endTime = Date.now()
        const duration = endTime - startTime

        console.log(`✅ Response (${duration}ms):`)
        console.log(`${result.text}`)

        if (result.toolResults && result.toolResults.length > 0) {
          console.log(`\n🔧 Tool Results:`)
          result.toolResults.forEach((toolResult, index) => {
            console.log(`  ${index + 1}. ${toolResult.toolName}: ${JSON.stringify(toolResult.result, null, 2)}`)
          })
        }

      } catch (error) {
        console.error(`❌ Error: ${error.message}`)
        if (error.stack) {
          console.error(`Stack: ${error.stack}`)
        }
      }

      // Add a small delay between requests
      await new Promise(resolve => setTimeout(resolve, 200))
    }

    console.log('\n🎉 All tests completed!')

  } catch (error) {
    console.error('💥 Failed to load agent, instructions, or test inputs:', error.message)
    if (error.stack) {
      console.error('Stack:', error.stack)
    }
    process.exit(1)
  }
}

// Run the tests
runTests().catch(console.error)
