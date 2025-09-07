import { readFile } from 'fs/promises'
import * as age from 'age-encryption'

/**
 * Decrypt an encrypted prompt using Age encryption with JWK private key
 */
async function decryptPrompt(encryptedContent: Buffer, privateKey: string): Promise<string> {
  try {
    // Parse JWK private key
    const privateKeyData = JSON.parse(privateKey) as JsonWebKey
    const identity = await crypto.subtle.importKey(
      'jwk',
      privateKeyData,
      { name: 'X25519' },
      true,
      ['deriveBits'],
    )

    const decrypter = new age.Decrypter()
    decrypter.addIdentity(identity)
    const decrypted = await decrypter.decrypt(new Uint8Array(encryptedContent), 'text')
    return decrypted
  } catch (error) {
    throw new Error(`Failed to decrypt prompt: ${error instanceof Error ? error.message : 'Unknown error'}`)
  }
}

/**
 * Load and decrypt an encrypted prompt file
 */
async function loadEncryptedPrompt(filePath: string, privateKey: string): Promise<string> {
  try {
    const encryptedContent = await readFile(filePath)
    return await decryptPrompt(encryptedContent, privateKey)
  } catch (error) {
    if (error instanceof Error && error.message.includes('ENOENT')) {
      throw new Error(`Encrypted prompt file not found: ${filePath}`)
    }
    throw error
  }
}

/**
 * Load encrypted prompt from environment configuration
 */
export async function loadPromptFromEnv(filePath: string): Promise<string> {
  const privateKey = process.env.PROMPT_PRIVATE_KEY
  if (privateKey === undefined || privateKey === null || privateKey.trim() === '') {
    throw new Error('PROMPT_PRIVATE_KEY environment variable is required for encrypted prompts')
  }

  try {
    return await loadEncryptedPrompt(filePath, privateKey)
  } finally {
    // Security: Clear the private key from memory
    const envKey = process.env.PROMPT_PRIVATE_KEY
    if (envKey !== undefined && envKey !== null && envKey.trim() !== '') {
      delete process.env.PROMPT_PRIVATE_KEY
    }
  }
}
