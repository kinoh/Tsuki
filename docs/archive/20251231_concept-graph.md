# Concept Graph Memory

## Overview
Introduce a concept-centric memory system backed by Memgraph and accessed via an external MCP service. Core remains responsible for LLM-based affect evaluation and for sending valence deltas, while the MCP service handles persistence, arousal updates, and recall ranking.

## Problem Statement
The existing structured-memory store is not well-suited for concept networks that combine affect values, episodic summaries, and semantic relations. We need a graph-first memory representation that is easy to query for associative recall and can evolve without tightly coupling Core to a specific database.

## Solution
- Use Memgraph as the graph database, managed via Docker Compose.
- Provide a Rust MCP service that exposes a minimal memory interface for Core.
- Keep affect evaluation in Core, and delegate arousal + recall scoring to the MCP service.
- Model concepts as the primary nodes, with episodes and relations attached to concepts.
- No user separation; the graph is shared.
- Data migration from the current structured-memory store is deferred.

## Design Decisions
- External MCP service instead of in-process storage to minimize Core dependencies and maintain clean layering.
- Affect values stored as latest properties on Concept nodes rather than separate nodes, to avoid unnatural graph shapes.
- Episodic memory stored as summarized text plus minimal metadata; accessed_at is tracked for arousal decay, while event time lives in the summary.
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
  - Episode node: name, summary, valence, arousal_level, accessed_at (event time is embedded in summary; accessed_at is for arousal decay)
  - Edges:
    - Concept->Concept with type in {is-a, part-of, evokes} and weight
    - EVOKES can connect Concept/Episode nodes (Concept->Episode, Episode->Concept, Episode->Episode)
- Compose integration
  - Add Memgraph service with persistent volume.
  - Core launches the MCP service binary via stdio; the MCP service connects to Memgraph via Bolt.

## Operations (Local)
- `task memgraph/local-clean` resets Memgraph data.
- `task memgraph/local-backup` creates a snapshot under `./backup/memgraph`.
- `task memgraph/local-restore/<snapshot>` or `task memgraph/local-restore/latest` restores Memgraph snapshots.
- `task local-reset` runs Memgraph restore + core data reset for a full local reset.

### MCP Interface (draft)
- General
  - Concept strings are used as-is (no normalization).
  - LLM-facing time is local time; Core converts to unix_ms before calling MCP.
  - MCP uses TZ to derive local dates for episode_id.
  - MCP ensures a unique constraint on Concept(name) at startup; startup fails if existing data violates it.
  - accessed_at is set by MCP; source and confidence are omitted.
  - Arousal is managed by MCP; Core only supplies valence deltas.
  - arousal = arousal_level * exp(-(now - accessed_at) / tau), with tau defaulting to 1 day.
  - recall scores are rounded to 6 decimal places for stability.
  - set_time is available only when launched with `--enable-set-time`.
    - now_ms <= 0 resets to real time.
- set_time
  - params: { now_ms: number }
  - returns: { now_ms: number | null, reset: boolean }
  - notes: internal clock override for backfill; not exposed without `--enable-set-time`.
- concept_upsert
  - params: { concept: string }
  - returns: { concept_id: string, created: boolean }
  - notes: newly created concepts start with arousal_level = 0.5.
- update_affect
  - params: { target: string, valence_delta: number }  # delta in [-1.0, 1.0]
  - returns: { concept_id or episode_id: string, valence: number, arousal: number, accessed_at: number }
  - notes: valence is clamped; accessed_at/arousal_level update only if new arousal >= current arousal.
  - notes: new arousal_level = abs(valence_delta).
  - notes: if target matches an Episode name, updates the episode; otherwise updates a concept (creating it if missing).
- episode_add
  - params: { summary: string, concepts: string[] }
  - returns: { episode_id: string, linked_concepts: string[], valence: number }
  - notes: concepts created indirectly here start with arousal_level = 0.25.
  - notes: episodes are created with valence = 0.0 and arousal_level = 0.5.
  - notes: episode_id is "YYYYMMDD/<keyword>" using the first concept as keyword; duplicates add "-2", "-3", etc.
  - notes: episode_id is stored as Episode.name in Memgraph for GUI visibility.
  - notes: episode_id is also de-duplicated against Concept names.
- relation_add
  - params: { from: string, to: string, type: "is-a" | "part-of" | "evokes" }
  - returns: { from: string, to: string, type: string }
  - notes: tautologies (from == to) are rejected.
  - notes: is-a / part-of are only allowed between Concepts.
  - notes: EVOKES is allowed between Concept/Episode nodes (Concept->Episode, Episode->Concept, Episode->Episode).
  - notes: concepts created indirectly here start with arousal_level = 0.25.
  - notes: relation weight starts at 0.25 and is strengthened on repeated relation_add
    (weight = 1 - (1 - weight) * (1 - 0.2)).
- concept_search
  - params: { keywords: string[], limit?: number }
  - returns: { concepts: string[] }
  - notes: limit defaults to 50 and maxes at 200.
  - notes: partial name match (case-insensitive); if insufficient, fills with arousal-ranked concepts.
- recall_query
  - params: { seeds: string[], max_hop: number }
  - returns: { propositions: Array<{ text: string, score: number, valence: number | null }> }
  - notes: relation types are mapped to DB-safe labels (e.g., "is-a" -> "IS_A"); propositions use a fixed
    text form, including episodes as "apple evokes <episode summary>".
  - notes: score = arousal * hop_decay * weight (for concept relations).
  - notes: hop_decay = 0.5^(hop-1); reverse relations apply a fixed 0.5 penalty.
  - notes: each recalled concept may update arousal_level using hop_decay if it raises arousal.
  - notes: scores are rounded to 6 decimals.

## Future Considerations
- Data migration strategy from structured-memory to Memgraph.
- Optional richer metadata on episodes (message ID) if recall quality requires it.
- Relation type expansion or normalization if concept graph usage grows.

## Initial Data Backfill (ad hoc)
- Run in an isolated environment/process from production to avoid clock/tool conflicts.
- Use `core/scripts/backfill_concept_graph.ts` to fetch message history for a specific user.
  - `pnpm tsx core/scripts/backfill_concept_graph.ts --user-id <id> --days-per-chunk <n> [--max-chunks <n>] [--since-chunk <n>]`
- Retrieve messages by thread (via `threadById`-equivalent flow) and process them in chunks.
- Keep the stepwise pipeline (整理 -> 検索 -> 概念/関係作成 -> エピソード作成 -> update_affect) to avoid long single-call timeouts.
- Introduce a `set_time` tool that is exposed only when a runtime option is explicitly provided
  (e.g., start the MCP server with `--enable-set-time`).
  - When used, `set_time` sets the internal clock to the chunk end timestamp (unix ms).
  - If not provided, the MCP service continues to use real-time `now`.
- Idempotency is not guaranteed; run after resetting the concept graph.
