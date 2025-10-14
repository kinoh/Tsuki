import { readFile, writeFile, readdir } from 'fs/promises'
import * as path from 'path'
import * as age from 'age-encryption'

// Decrypt prompt file using Age encryption

async function main() {
  const promptsDir = 'src/prompts'
  const files = await readdir(promptsDir)
  const encryptedFiles = files.filter(f => f.endsWith('.txt.encrypted'))

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

    const d = new age.Decrypter()
    d.addIdentity(identity)

    for (const encFile of encryptedFiles) {
      const inputFile = path.join(promptsDir, encFile)
      const outputFile = path.join(
        promptsDir,
        encFile.replace(/\.txt\.encrypted$/, '.txt')
      )
      try {
        const encrypted = await readFile(inputFile)
        const plaintext = await d.decrypt(encrypted, "text")
        await writeFile(outputFile, plaintext, 'utf8')
        console.log(`Decrypted ${inputFile} -> ${outputFile}`)
      } catch (err) {
        console.error(`Decryption failed for ${inputFile}:`, err.message)
      }
    }
  } catch (error) {
    console.error('Decryption failed:', error.message)
    process.exit(1)
  }
}

main()
