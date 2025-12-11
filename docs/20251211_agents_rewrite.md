# AGENTS.md rewrite

## Rationale
- Align agent-facing guidance with the product concept: casual, playful chat with a pre-conscious router that may forward repeated sensory inputs to surface new facets, favoring liveliness over determinism.
- Reduce duplication by pointing HTTP details to `core/src/server/routes` and WebSocket protocol to the AsyncAPI spec instead of re-listing endpoints.
- Summarize MCP servers by shared characteristics (Rust binaries, `DATA_DIR`-backed storage, per-user isolation) rather than enumerating every detail.

## Decisions
- Replaced `AGENTS.md` with a concise, concept-first document covering: concept & experience, system shape, interfaces (HTTP/WebSocket/Admin), MCP topology, runtime commands, config/data, and testing notes.
- Document now references `api-specs/asyncapi.yaml` for WebSocket protocol and `core/src/server/routes` for HTTP routes; removed duplicated endpoint listings.
- MCP section highlights universal RSS MCP (npm) and per-user Rust MCPs (`scheduler`, `structured-memory`) with storage roots under `DATA_DIR`.
- Second pass: added directory layout (core/gui/api-specs/docs/docker) and restructured bullets hierarchically to keep single ideas per line.

## Notes
- No code or API behavior changes; documentation-only update.
