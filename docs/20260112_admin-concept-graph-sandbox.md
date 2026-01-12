# Admin Concept Graph & Sandbox Memory

## Overview
Extend the core AdminJS panel to remove structured-memory and add read-only views for concept graph data and sandbox /memory files. The admin path should query storage directly, not through MCP, to avoid LLM-oriented constraints.

## Problem Statement
- structured-memory was removed and should no longer appear in the admin UI.
- Operators need to inspect Concept, Episode, and Relation data from the concept graph.
- Operators need to browse and read files under the sandbox `/memory` volume from the admin UI.

## Solution
- Remove the structured-memory AdminJS resource and navigation.
- Add AdminJS resources for Concept, Episode, and Relation, backed by direct Memgraph queries.
- Add an AdminJS resource for sandbox `/memory` that lists files recursively and shows contents.

## Design Decisions
- Query concept graph directly via Bolt (Memgraph) instead of MCP to keep the admin path independent of the LLM tool interface.
- Keep admin resources read-only (no create/edit/delete) to avoid unintended data changes.
- Mount the sandbox volume into the core service as read-only so `/memory` is readable from the admin backend.
- Read prompt memory directly from `/memory` instead of using `shell_exec` to keep core independent of the sandbox tool.
- Use `neo4j-driver` for Bolt access from the admin backend.
- Truncate sandbox file content at 128 KB with a warning to avoid oversized admin payloads.

## Implementation Details
- Add a Bolt client (`neo4j-driver`) in core to query Memgraph using `MEMGRAPH_URI`, `MEMGRAPH_USER`, and `MEMGRAPH_PASSWORD`.
- Provide three resources:
  - Concept: name, valence, arousal_level, accessed_at.
  - Episode: name, summary, valence, arousal_level, accessed_at.
  - Relation: from, to, type, weight.
- Add a sandbox memory resource that:
  - Recursively lists files under `/memory`.
  - Shows file content, size, and modified time.
  - Truncates content over 128 KB and shows a warning header.
  - Blocks edits and deletes.
- Update `compose.yaml` to mount `sandbox-data` at `/memory:ro` in the core container so the admin backend can read the files safely.

## Future Considerations
- Add content size limits or streaming for very large files if admin responsiveness becomes an issue.
