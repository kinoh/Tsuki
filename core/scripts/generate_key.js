import { writeFile } from 'fs/promises'

// Generate key pair in JWK format for encryption

// Generate X25519 key pair for Age encryption
const keyPair = await crypto.subtle.generateKey(
  { name: 'X25519' },
  true, // extractable
  ['deriveBits']
)

// Export as JWK (text format)
const privateKeyJWK = await crypto.subtle.exportKey('jwk', keyPair.privateKey)
const publicKeyJWK = await crypto.subtle.exportKey('jwk', keyPair.publicKey)

// Save keys to files
await writeFile('./private_key.jwk', JSON.stringify(privateKeyJWK, null, 2))
await writeFile('./public_key.jwk', JSON.stringify(publicKeyJWK, null, 2))

console.log('Private Key JWK:', JSON.stringify(privateKeyJWK, null, 2))
console.log('Public Key JWK:', JSON.stringify(publicKeyJWK, null, 2))
console.log('Keys saved to ./private_key.jwk and ./public_key.jwk')
