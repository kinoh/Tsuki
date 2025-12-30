# Investigation notes: reasoning model support (OpenAI Responses item_reference)

## Background
- When OpenAI Responses returns reasoning, the follow-up request fails with:
  - `Item 'rs_...' of type 'reasoning' was provided without its required following item.`
- In the DB, the reasoning part's `itemId` is saved, but the text part's `itemId` is not.

## How we narrowed it down (why we traced the full code path)
- **Initial hypothesis**: `providerMetadata` might be dropped during DB save.
- **First attempt**: add a patch to save `providerMetadata` in `aiV5ModelMessageToMastraDBMessage`.
- **Result**: no fix. The text part still lacks `itemId`.
- **Next hypothesis**: v5 conversion might not be used.
- **Check**: type-check logs showed `isAIV4CoreMessage=true` and `isAIV5CoreMessage=false`, so it was handled as v4.
- **Try**: forcing `isAIV4CoreMessage` to `false` (treat as v5) still did not fix it.
- **Conclusion**: data was likely lost before/around streaming, not only at v4/v5 conversion.
- **Decision**: trace the full path with logs: AI SDK -> v5 transform -> Mastra process -> DB save.

## Investigation method
- Trace the path from AI SDK -> Mastra transform -> DB save.
- Add logs to v5 transform and process streaming to observe `text-start`/`text-delta`/`text-end` `providerMetadata` and `runState` changes.
- Check which `providerMetadata` is used when flushing on `text-end`.

## What we learned
- The v5 transform path is actually used (logs confirmed `text-start`/`text-delta`/`text-end`).
- `text-start` carries `providerMetadata.openai.itemId` (`msg_...`), but `text-delta`/`text-end` can have no `providerMetadata`.
- `processOutputStream` ignores `text-start`/`text-end`, so `runState.providerOptions` is not updated.
- When flushing on `text-end`, `providerMetadata` is `undefined`, and the text part loses `itemId`.

## Root cause
- `processOutputStream` does not preserve `providerMetadata` from `text-start`/`text-end`, so `msg_...` is not kept in `runState`.
- If `text-delta` lacks `providerMetadata`, the DB message for the text part has no `itemId`.

## Fix applied
- Add handling for `text-start`/`text-end` in `processOutputStream` to save `providerMetadata` into `runState.providerOptions`.
- Applied in:
  - `core/patches/@mastra__core@1.0.0-beta.18.patch`
  - `dist/chunk-GFALVOBW.cjs` (CJS)
  - `dist/chunk-2XX35XRX.js` (ESM)
- Minimal change:
  - preserve `providerMetadata` on `text-start` and `text-end`, pass through chunks as-is.

## Minimal Reproduction
Prerequisites:
- Memory enabled in Mastra
- OpenAI Responses reasoning model (e.g. `gpt-5.1-chat-latest` / `o1` / `o3`)

Steps:
1. Send a prompt that triggers reasoning (e.g., "Think step by step: 2+2").
2. Immediately send any follow-up message (e.g., "Hello").
3. The second request fails with:
   `Item 'rs_...' of type 'reasoning' was provided without its required following item.`

Observed state:
- In the DB assistant message, the reasoning part has `providerMetadata.openai.itemId=rs_...`.
- The text part does not have `providerMetadata.openai.itemId=msg_...`.
- On replay, only `rs_...` is sent and `msg_...` is missing, so OpenAI rejects it.
