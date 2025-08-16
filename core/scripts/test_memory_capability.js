#!/usr/bin/env node

/**
 * Memory Capability Test Script
 * Tests 3 different memory patterns with book list scenario
 */

import { readFile } from 'fs/promises'
import { resolve } from 'path'
import { fileURLToPath } from 'url'
import { dirname } from 'path'
import { parse as parseYaml } from 'yaml'
import dotenv from 'dotenv'
import { openai } from '@ai-sdk/openai'
import { Agent } from '@mastra/core/agent'
import { Memory } from '@mastra/memory'
import { LibSQLStore, LibSQLVector } from '@mastra/libsql'

// Load environment variables
dotenv.config()

const __filename = fileURLToPath(import.meta.url)
const __dirname = dirname(__filename)

// Configuration
const dataDir = process.env.DATA_DIR ?? './data'
const dbPath = `file:${dataDir}/mastra.db`
const openAiModel = process.env.OPENAI_MODEL ?? 'gpt-4o-mini'
const testUserId = 'test-user'

console.log(`🧪 Memory Capability Test`)
console.log(`dataDir: ${dataDir}`)
console.log(`testDbPath: ${dbPath}`)
console.log(`openAiModel: ${openAiModel}`)
console.log('')

/**
 * Load encrypted prompt instructions
 */
async function loadInstructions() {
  try {
    const { loadPromptFromEnv } = await import('../src/prompt.ts')
    return await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
  } catch (error) {
    throw new Error(`Failed to load prompt: ${error}`)
  }
}

/**
 * Load test scenarios from YAML file
 */
async function loadTestScenarios() {
  const scenariosPath = resolve(__dirname, './test_memory_scenarios.yaml')
  const data = await readFile(scenariosPath, 'utf-8')
  const yamlData = parseYaml(data)
  return yamlData
}

/**
 * Semantic Memory: Mastra Memory Only
 * lastMessages=3, semanticRecall enabled
 */
async function createSemanticMemory(instructions) {
  const memory = new Memory({
    storage: new LibSQLStore({
      url: dbPath,
    }),
    vector: new LibSQLVector({
      connectionUrl: dbPath,
    }),
    embedder: openai.embedding('text-embedding-3-small'),
    options: {
      lastMessages: 3, // Short for API cost efficiency
      semanticRecall: {
        topK: 5,
        messageRange: 2,
        scope: 'resource',
      },
    },
  })

  return new Agent({
    name: 'semantic-memory',
    instructions: ({ runtimeContext }) => {
      const contextInstructions = runtimeContext.get('instructions')
      return contextInstructions || instructions
    },
    model: openai(openAiModel),
    memory,
  })
}

/**
 * External Storage: Mastra Memory + Notion MCP
 * lastMessages=3, semanticRecall + Notion external storage
 */
async function createExternalStorage(instructions) {
  const memory = new Memory({
    storage: new LibSQLStore({
      url: dbPath,
    }),
    vector: new LibSQLVector({
      connectionUrl: dbPath,
    }),
    embedder: openai.embedding('text-embedding-3-small'),
    options: {
      lastMessages: 3,
      semanticRecall: {
        topK: 5,
        messageRange: 2,
        scope: 'resource',
      },
    },
  })

  const { getDynamicMCP } = await import('../src/mastra/mcp.ts')

  return new Agent({
    name: 'external-storage',
    instructions: ({ runtimeContext }) => {
      const contextInstructions = runtimeContext.get('instructions')
      const baseInstructions = contextInstructions || instructions
      return baseInstructions + '\\n\\nYou have access to Notion tools for external memory storage. Use create-pages to save important information and search to retrieve it.'
    },
    model: openai(openAiModel),
    memory,
    tools: await getDynamicMCP(testUserId, async (server, url) => {
      console.log(`Please authenticate with ${server} at ${url}`);
    }).getTools(),
  })
}

/**
 * Working Memory: Mastra Memory + Working Memory
 * lastMessages=3, semanticRecall + workingMemory enabled
 */
async function createWorkingMemory(instructions) {
  const memory = new Memory({
    storage: new LibSQLStore({
      url: dbPath,
    }),
    vector: new LibSQLVector({
      connectionUrl: dbPath,
    }),
    embedder: openai.embedding('text-embedding-3-small'),
    options: {
      lastMessages: 3, // Same as other patterns
      semanticRecall: {
        topK: 5,
        messageRange: 2,
        scope: 'resource',
      },
      workingMemory: true, // Enable Mastra's working memory
    },
  })

  return new Agent({
    name: 'working-memory',
    instructions: ({ runtimeContext }) => {
      const contextInstructions = runtimeContext.get('instructions')
      const baseInstructions = contextInstructions || instructions
      return baseInstructions + '\\n\\nYou have enhanced working memory capabilities that help you remember information within conversations.'
    },
    model: openai(openAiModel),
    memory,
  })
}

/**
 * Execute single test with an agent
 */
async function executeTest(agent, message, context, testName) {
  console.log(`    💬 Input: "${message}"`)
  
  try {
    const startTime = Date.now()
    
    const result = await agent.generate([{
      role: 'user',
      content: message
    }], {
      runtimeContext: context,
      memory: {
        resource: testUserId,
        thread: `test_${testName}_${Date.now()}`,
        options: {
          lastMessages: 3, // Consistent with agent config
        },
      },
    })

    const endTime = Date.now()
    const duration = endTime - startTime

    console.log(`    ✅ Response (${duration}ms):`)
    console.log(`    ${result.text}`)

    if (result.toolResults && result.toolResults.length > 0) {
      console.log(`    🔧 Tool Results:`)
      result.toolResults.forEach((toolResult, index) => {
        console.log(`      ${index + 1}. ${toolResult.toolName}: ${JSON.stringify(toolResult.result, null, 2)}`)
      })
    }

    return {
      success: true,
      response: result.text,
      duration,
      toolResults: result.toolResults || [],
    }

  } catch (error) {
    console.error(`    ❌ Error: ${error.message}`)
    return {
      success: false,
      error: error.message,
      response: '',
      duration: 0,
      toolResults: [],
    }
  }
}

/**
 * Run memory capability test for all patterns
 */
async function runMemoryTests() {
  console.log('🤖 Loading instructions and test scenarios...')

  try {
    const instructions = await loadInstructions()
    const scenarios = await loadTestScenarios()
    
    console.log(`📝 Loaded test scenario: ${scenarios.memory_test ? 'Book List Memory Test' : 'Unknown'}`)
    console.log('')

    // Create runtime context
    const runtimeContext = new Map()
    runtimeContext.set('instructions', instructions)

    // Create agents for each pattern
    console.log('🔧 Creating test agents...')
    const semanticMemory = await createSemanticMemory(instructions)
    const externalStorage = await createExternalStorage(instructions)
    const workingMemory = await createWorkingMemory(instructions)

    const patterns = [
      { name: 'semantic-memory', agent: semanticMemory },
      { name: 'external-storage', agent: externalStorage },
      { name: 'working-memory', agent: workingMemory },
    ]

    console.log('')

    // Test each pattern
    for (const pattern of patterns) {
      console.log(`${'='.repeat(60)}`)
      console.log(`🧪 Testing ${pattern.name}`)
      console.log(`${'='.repeat(60)}`)

      // Phase 1: Memory Input
      console.log('📚 Phase 1: Memory Input')
      await executeTest(
        pattern.agent,
        scenarios.memory_test.memory_input,
        runtimeContext,
        `${pattern.name.replace(/\\s+/g, '_')}_memory`
      )

      // Small delay between phases
      await new Promise(resolve => setTimeout(resolve, 100))

      // Phase 2: Filler Messages
      console.log('\\n🔄 Phase 2: Filler Messages (to exceed lastMessages=3)')
      for (let i = 0; i < scenarios.memory_test.fillers.length; i++) {
        const filler = scenarios.memory_test.fillers[i]
        console.log(`  Filler ${i + 1}/${scenarios.memory_test.fillers.length}`)
        
        await executeTest(
          pattern.agent,
          filler,
          runtimeContext,
          `${pattern.name.replace(/\\s+/g, '_')}_filler_${i}`
        )

        // Short delay between fillers
        await new Promise(resolve => setTimeout(resolve, 100))
      }

      // Phase 3: Memory Recall Test
      console.log('\\n🧠 Phase 3: Memory Recall Test')
      const test = scenarios.memory_test.recall_test
      console.log(`  Query: ${test.query}`)
      
      const recallResult = await executeTest(
        pattern.agent,
        test.query,
        runtimeContext,
        `${pattern.name.replace(/\\s+/g, '_')}_recall`
      )

      // Check if expected keywords are present
      const responseText = recallResult.response.toLowerCase()
      const foundKeywords = test.expected_keywords.filter(keyword => 
        responseText.includes(keyword.toLowerCase())
      )

      const keywordMatch = foundKeywords.length > 0
      
      console.log(`  📊 Expected Keywords: [${test.expected_keywords.join(', ')}]`)
      console.log(`  🔍 Found Keywords: [${foundKeywords.join(', ')}]`)
      console.log(`  ${keywordMatch ? '✅ SUCCESS' : '❌ FAILED'}: Keyword matching`)

    }

    console.log(`${'='.repeat(60)}`)
    console.log('🎉 Memory capability testing completed!')
    console.log(`${'='.repeat(60)}`)

  } catch (error) {
    console.error('💥 Failed to run memory tests:', error.message)
    if (error.stack) {
      console.error('Stack:', error.stack)
    }
    process.exit(1)
  }
}

// Run the tests
runMemoryTests().catch(console.error)