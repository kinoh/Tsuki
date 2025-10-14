import { readFile, writeFile, readdir } from 'fs/promises'
import * as path from 'path'
import * as age from 'age-encryption'

// Encrypt prompt file using RSA-OAEP with JWK key

async function main() {
  const promptsDir = 'src/prompts'
  const files = await readdir(promptsDir)
  // Exclude files that are already encrypted or not .txt
  const txtFiles = files.filter(f => f.endsWith('.txt') && !f.endsWith('.txt.encrypted'))

  const privateKeyJWK = process.env.PROMPT_PRIVATE_KEY
  if (!privateKeyJWK) {
    console.error('PROMPT_PRIVATE_KEY environment variable is required')
    process.exit(1)
  }

  try {
    const privateKeyData = JSON.parse(privateKeyJWK)
    const identity = await crypto.subtle.importKey(
      "jwk",
      privateKeyData,
      { name: "X25519" },
      true,
      ["deriveBits"],
    )
    const recipient = await age.identityToRecipient(identity)
    const e = new age.Encrypter()
    e.addRecipient(recipient)

    for (const txtFile of txtFiles) {
      const inputFile = path.join(promptsDir, txtFile)
      const outputFile = path.join(promptsDir, txtFile + '.encrypted')
      try {
        const plaintext = await readFile(inputFile, 'utf8')
        const encrypted = await e.encrypt(plaintext)
        await writeFile(outputFile, encrypted)
        console.log(`Encrypted ${inputFile} -> ${outputFile}`)
      } catch (err) {
        console.error(`Encryption failed for ${inputFile}:`, err.message)
      }
    }
  } catch (error) {
    console.error('Encryption failed:', error.message)
    process.exit(1)
  }
}

main()
