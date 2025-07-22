import { readFile, writeFile } from 'fs/promises'
import * as age from 'age-encryption'

// Encrypt prompt file using RSA-OAEP with JWK key

async function main() {
  const inputFile = 'src/prompts/initial.txt'
  const outputFile = 'src/prompts/initial.txt.encrypted'

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

    const plaintext = await readFile(inputFile, 'utf8')
    const encrypted = await e.encrypt(plaintext)

    await writeFile(outputFile, encrypted)
    console.log(`Encrypted ${inputFile} -> ${outputFile}`)

  } catch (error) {
    console.error('Encryption failed:', error.message)
    process.exit(1)
  }
}

main()
