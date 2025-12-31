# Concept Graph Memory

## Overview
Introduce a concept-centric memory system backed by Memgraph and accessed via an external MCP service. Core remains responsible for LLM-based affect evaluation and for supplying scoring criteria, while the MCP service handles persistence and graph queries.

## Problem Statement
The existing structured-memory store is not well-suited for concept networks that combine affect values, episodic summaries, and semantic relations. We need a graph-first memory representation that is easy to query for associative recall and can evolve without tightly coupling Core to a specific database.

## Solution
- Use Memgraph as the graph database, managed via Docker Compose.
- Provide a Rust MCP service that exposes a minimal memory interface for Core.
- Keep affect evaluation and recall scoring criteria in Core, and pass computed criteria to the MCP service for ranking.
- Model concepts as the primary nodes, with episodes and relations attached to concepts.
- No user separation; the graph is shared.
- Data migration from the current structured-memory store is deferred.

## Design Decisions
- External MCP service instead of in-process storage to minimize Core dependencies and maintain clean layering.
- Affect values stored as latest properties on Concept nodes rather than separate nodes, to avoid unnatural graph shapes.
- Episodic memory stored as summarized text plus minimal metadata (timestamp, source).
- Fixed concept relation types: is-a, part-of, evokes.
  - **is-a** is an asymmetric inclusion relation grounded in similarity; it encodes abstraction level and inheritance
    with minimal notation, enabling compact taxonomy and generalization queries.
  - **part-of** preserves component-to-whole correspondence; as a base relation it supports profiling and perspective
    shifts while enabling structural retrieval and composition-aware queries.
  - **evokes** marks directed recall paths without assuming causality; it underpins associative access and provides
    a stable handle for cue-based ranking in retrieval.
- Concept normalization and synonym merging are out of scope for this change; Core (or a worker) may handle it later.

## Implementation Details
- Core responsibilities
  - Run LLM-based affect evaluation from conversation logs.
  - Send concept updates, episodic summaries, and scoring criteria to the MCP service.
  - Request recall queries and consume ranked results.
- MCP memory service (Rust)
  - Translate Core requests into Memgraph writes and reads.
  - Apply ranking using scoring criteria provided by Core.
- Graph model (logical)
  - Concept node: identifier (concept text), latest affect values, updated timestamp, source
  - Episode node: summary, timestamp, source
  - Edges: Concept->Episode (mentions/related), Concept->Concept with type in {is-a, part-of, evokes}
- Compose integration
  - Add Memgraph service with persistent volume.
  - Add MCP service that connects to Memgraph via Bolt.

## Future Considerations
- Data migration strategy from structured-memory to Memgraph.
- Optional richer metadata on episodes (message ID, confidence) if recall quality requires it.
- Relation type expansion or normalization if concept graph usage grows.
