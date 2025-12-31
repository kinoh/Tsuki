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
  - Send concept updates, episodic summaries, and affect deltas to the MCP service.
  - Request recall queries and consume ranked results.
- MCP memory service (Rust)
  - Translate Core requests into Memgraph writes and reads.
  - Apply ranking internally; manage arousal updates; set updated timestamps.
- Graph model (logical)
  - Concept node: identifier (concept text), valence, arousal_level, accessed_at
  - Episode node: summary, valence (time is embedded in summary or represented elsewhere, not as a timestamp property)
  - Edges: Concept->Episode (mentions/related), Concept->Concept with type in {is-a, part-of, evokes}
- Compose integration
  - Add Memgraph service with persistent volume.
  - Add MCP service that connects to Memgraph via Bolt.

### MCP Interface (draft)
- General
  - Concept strings are used as-is (no normalization).
  - LLM-facing time is local time; Core converts to unix_ms before calling MCP.
  - accessed_at is set by MCP; source and confidence are omitted.
  - Arousal is managed by MCP and not explicitly updated by Core.
  - arousal = arousal_level * exp(-(now - accessed_at) / tau), with tau defaulting to 1 day.
- concept_upsert
  - params: { concept: string }
  - returns: { concept_id: string, created: boolean }
- concept_update_affect
  - params: { concept: string, valence_delta: number }  # delta in [-1.0, 1.0]
  - returns: { concept_id: string, valence: number, arousal: number, accessed_at: number }
  - notes: valence is clamped; accessed_at/arousal_level update only if new arousal >= current arousal.
- episode_add
  - params: { summary: string, concepts: string[], valence: number }
  - returns: { episode_id: string, linked_concepts: string[], valence: number }
- relation_add
  - params: { from: string, to: string, type: "is-a" | "part-of" | "evokes" }
  - returns: { relation_id: string }
- recall_query
  - params: { seeds: string[], max_hop: number }
  - returns: { propositions: Array<{ text: string, score: number, valence: number | null }> }
  - notes: relation types are mapped to DB-safe labels (e.g., "is-a" -> "IS_A"); propositions use a fixed
    text form, including episodes as "apple evokes <episode summary>".
  - notes: each recalled concept may update arousal_level using hop_decay if it raises arousal.
  - notes: hop_decay is directional (forward: 0.5^(hop-1), reverse: 0.5^hop).

## Future Considerations
- Data migration strategy from structured-memory to Memgraph.
- Optional richer metadata on episodes (message ID) if recall quality requires it.
- Relation type expansion or normalization if concept graph usage grows.
