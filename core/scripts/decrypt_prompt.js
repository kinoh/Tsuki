import { readFile, writeFile } from 'fs/promises'
import * as age from 'age-encryption'

// Decrypt prompt file using Age encryption

async function main() {
  const inputFile = 'src/prompts/initial.txt.encrypted'
  const outputFile = 'src/prompts/initial.txt'

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

    const encrypted = await readFile(inputFile)
    const plaintext = await d.decrypt(encrypted, "text")

    await writeFile(outputFile, plaintext, 'utf8')
    console.log(`Decrypted ${inputFile} -> ${outputFile}`)

  } catch (error) {
    console.error('Decryption failed:', error.message)
    process.exit(1)
  }
}

main()
