# Memory Seed Tool from Backup Mastra DB

## Context
- A lightweight utility is needed to generate initial memory content from backup archives.
- Source data should come from `mastra_messages` inside `mastra.db` contained in `tsuki-backup-*.tar.gz`.
- Input to generation should be:
  - extracted message history
  - a user-specified prompt

## Decisions
- Added `core/scripts/generate_memory_seed.ts`.
- Added npm script:
  - `pnpm run memory:seed`
- Backup handling:
  - default uses latest `../backup/tsuki-backup-*.tar.gz`
  - supports explicit `--backup <path>`
  - supports `--cache-dir <path>` (default: `/tmp/tsuki-mastra-cache`)
  - extracts only `mastra.db` into the cache directory
  - reuses extracted db on cache hit (same backup filename + mtime + size)
- Message loading:
  - reads from `mastra_messages`
  - supports optional filters: `--thread-id`, `--resource-id`
  - uses `--limit` latest messages (descending query then reversed for chronological input)
- Prompt input:
  - `--prompt "<text>"` or `--prompt-file <path>`
  - prompt is optional when `--dry-run` is set
- Generation:
  - uses OpenAI via existing `@ai-sdk/openai`
  - system prompt is the provided prompt
  - user content is full simplified history text
  - output extraction is resilient:
    - first tries `result.text`
    - falls back to `result.content[type=text]`
    - falls back to `response.body.output[].content[type=output_text]`
- Safety / validation:
  - `--dry-run` mode prints the full history payload that would be sent to the LLM, without model calls
  - requires `OPENAI_API_KEY` only when generation is requested
- History cleanup policy:
  - exclude system messages
  - exclude the immediate assistant response to a system message
  - additionally treat infrastructure notifications as system-equivalent messages
    - lines starting with `[source:MCP:`
    - lines starting with `Received scheduler notification:`

## Why
- Reuses existing project dependencies and scripting style (`tsx`).
- Keeps extraction and query logic local and explicit.
- Enables iterative prompt design with dry-run history inspection before spending tokens.

## Usage Examples
- Dry run with latest backup:
  - `cd core && pnpm run memory:seed -- --prompt-file ./scripts/memory_seed_prompt.txt --dry-run`
- Generate from specific backup and thread:
  - `cd core && pnpm run memory:seed -- --backup ../backup/tsuki-backup-20260208231014.tar.gz --thread-id kino-20260205 --prompt-file ./scripts/memory_seed_prompt.txt --output ./data/memory_seed.md`
- Use custom cache directory:
  - `cd core && pnpm run memory:seed -- --cache-dir /tmp/tsuki-cache --prompt-file ./scripts/memory_seed_prompt.txt --dry-run`
