# Mastra upgrade adjustments

## Decisions
- Removed `Metric` from Mastra/Agent type parameters because it is not used directly (per user clarification) and the type no longer exists in the upgraded Mastra exports.
- Switched Mastra imports to their new module paths (`@mastra/core/agent`, `@mastra/core/memory`, `@mastra/core/tools`, `@mastra/core/storage`, `@mastra/core/request-context`) since the root export now only exposes `Mastra`.
- Updated memory access to the new API (`recall`, `listThreadsByResourceId`) and standardized message types to `MastraDBMessage`.
- Replaced per-user `RuntimeContext` usage with `RequestContext` for agent instructions and generation options.
- Updated MCP access to the new tool APIs (`listTools`, `listToolsets`) and direct tool execution signature.
- Added required `id` fields for LibSQL storage/vector configuration as per new config types.
- Added direct `ai` dependency aligned with `@ai-sdk/openai@3.x` to fix model type mismatches in `generateText`.

## Open item
- `ai` package version is incompatible with `@ai-sdk/openai@3.x` (LanguageModelV1 vs V3). This needs a dependency alignment decision before changing versions.
