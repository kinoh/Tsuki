# Structured Memory Empty Input Schema

## Decision
- Add an explicit empty request type for the `get_document_tree` MCP tool so its generated `inputSchema` is a JSON object.

## Rationale
- The MCP SDK validates `inputSchema.type` as `object`; an explicit empty request ensures the schema is emitted as an object even for no-arg tools.

## Scope
- `mcp/structured-memory/src/service.rs`

