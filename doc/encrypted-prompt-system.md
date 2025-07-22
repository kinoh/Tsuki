# Encrypted Prompt System

## Overview

The Mastra backend now supports encrypted prompt files using Age encryption with X25519 key pairs. This provides secure storage and runtime loading of agent instructions and prompts.

## Architecture

### Encryption Method
- **Algorithm**: Age encryption with X25519 key pairs
- **Key Format**: JWK (JSON Web Key) format
- **Library**: `age-encryption` npm package
- **Web Crypto API**: Native X25519 support in Node.js

### Components

```
core/
├── src/
│   └── prompt.ts             # Age decryption with JWK support
├── scripts/
│   ├── generate_key.js       # X25519 key pair generation
│   ├── encrypt_prompt.js     # Age encryption script
│   ├── decrypt_prompt.js     # Age decryption script
│   └── ws_client.js          # WebSocket test client
└── src/prompts/
    ├── initial.txt           # Plaintext prompt (gitignored)
    └── initial.txt.encrypted # Encrypted prompt file
```

## Key Generation

Generate X25519 key pairs in JWK format:

```bash
cd core/
node scripts/generate_key.js
```

**Output:**
- `private_key.jwk` - Private key (keep secure)
- `public_key.jwk` - Public key (for reference)

**JWK Format:**
```json
{
  "kty": "OKP",
  "crv": "X25519",
  "d": "private_key_bytes_base64",
  "x": "public_key_bytes_base64",
  "key_ops": ["deriveBits"],
  "ext": true
}
```

## Encryption/Decryption Scripts

### Encryption
```bash
# Add PROMPT_PRIVATE_KEY to .env file first
node --env-file .env scripts/encrypt_prompt.js
```
- **Input**: `src/prompts/initial.txt`
- **Output**: `src/prompts/initial.txt.encrypted`

### Decryption (for verification)
```bash
node --env-file .env scripts/decrypt_prompt.js
```
- **Input**: `src/prompts/initial.txt.encrypted`
- **Output**: `src/prompts/initial.txt.decrypted`

## Runtime Integration

### Environment Configuration
```bash
# Required for encrypted prompt loading
PROMPT_PRIVATE_KEY='{"kty":"OKP","crv":"X25519",...}'
```

### Code Integration

The system automatically loads encrypted prompts at startup:

```typescript
// src/index.ts
async function createRuntimeContext(): Promise<RuntimeContext<AppRuntimeContext>> {
  const runtimeContext = new RuntimeContext<AppRuntimeContext>()
  
  try {
    const instructions = await loadPromptFromEnv('src/prompts/initial.txt.encrypted')
    runtimeContext.set('instructions', instructions)
  } catch (error) {
    console.warn('Failed to load encrypted prompt, using fallback:', error)
    runtimeContext.set('instructions', 'You are a helpful chatting agent.')
  }

  return runtimeContext
}
```

### Agent Configuration

Agents use runtime context for dynamic instructions:

```typescript
// src/mastra/agents/tsuki.ts
export const tsuki = new Agent({
  name: 'Tsuki',
  instructions: ({ runtimeContext }): string => {
    const instructions = runtimeContext.get('instructions')
    if (instructions === null || instructions === undefined || instructions === '') {
      console.warn('Instructions not found in runtime context, using default instructions')
      return 'You are a helpful chatting agent.'
    }
    return instructions as string
  },
  // ...
})
```

## Implementation Details

### Prompt Decryption (`src/prompt.ts`)

```typescript
export async function decryptPrompt(encryptedContent: Buffer, privateKey: string): Promise<string> {
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
```

### Key Features

1. **JWK Integration**: Direct Web Crypto API support
2. **Age Encryption**: Industry-standard encryption for files
3. **Fallback Mechanism**: Graceful degradation to default prompts
4. **Environment Security**: Keys loaded from environment variables
5. **Runtime Loading**: Dynamic prompt loading at startup

## Security Considerations

### Key Management
- **Private Keys**: Store securely, never commit to repository
- **Environment Variables**: Use secure environment variable management
- **Key Rotation**: Generate new key pairs regularly
- **Access Control**: Restrict access to private key files

### File Security
- **Gitignore**: Plaintext prompts are gitignored
- **Encryption**: Only encrypted files are committed
- **Verification**: Decryption scripts for verification only

### Runtime Security
- **Memory Clearing**: Private keys cleared from environment after use
- **Error Handling**: Sanitized error messages
- **Fallback**: Default instructions if decryption fails

## Docker Integration

The Docker image automatically supports encrypted prompts:

```bash
# Run with encrypted prompts
docker run -e PROMPT_PRIVATE_KEY='{"kty":"OKP",...}' tsuki-core
```

**Key Points:**
- No Dockerfile changes needed
- Dependencies automatically installed via npm
- Environment variables passed at runtime
- Encrypted files included in image

## Development Workflow

### Initial Setup
1. Generate X25519 key pair: `node scripts/generate_key.js`
2. Create plaintext prompt: `src/prompts/initial.txt`
3. Add private key to `.env` file: `PROMPT_PRIVATE_KEY='{"kty":"OKP",...}'`
4. Encrypt prompt: `node --env-file .env scripts/encrypt_prompt.js`

### Updating Prompts
1. Edit `src/prompts/initial.txt`
2. Re-encrypt: `node --env-file .env scripts/encrypt_prompt.js`
3. Commit encrypted file: `git add src/prompts/initial.txt.encrypted`

### Testing
```bash
# Test WebSocket with encrypted prompts
npm start

# Test client
node scripts/ws_client.js
```

## Advantages

1. **Security**: Prompts encrypted at rest
2. **Simplicity**: JWK format integrates with Web Crypto API
3. **Compatibility**: Age encryption is widely supported
4. **Performance**: Minimal runtime overhead
5. **Flexibility**: Easy to add more encrypted resources

## Migration from Legacy Systems

For systems migrating from other encryption methods:

1. **SSH Keys**: Previous SSH-to-Age conversion logic removed for simplicity
2. **JWK Standard**: Uses standard JWK format instead of custom encodings
3. **Age Library**: Leverages mature age-encryption ecosystem
4. **Web Crypto**: Native Node.js cryptographic support

## Troubleshooting

### Common Issues

1. **Invalid JWK Format**: Ensure proper JSON structure
2. **Missing Environment Variable**: Set `PROMPT_PRIVATE_KEY`
3. **Decryption Failures**: Verify key pair matches encrypted file
4. **File Not Found**: Ensure encrypted file exists at expected path

### Debug Mode
```bash
# Enable detailed error logging (add DEBUG=1 to .env file)
npm start
```

## Future Enhancements

- **Key Rotation**: Automated key rotation scripts
- **Multiple Prompts**: Support for multiple encrypted prompt files
- **Key Derivation**: PBKDF2 key derivation from passwords
- **HSM Integration**: Hardware security module support
- **Audit Logging**: Prompt access and decryption logging