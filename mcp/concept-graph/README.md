# Concept graph MCP server

Memgraph-backed concept graph storage for concept updates, relations, episodic summaries,
and recall queries. This server is intended to be used via MCP stdio and queried by
LLM-driven agents.

## Configuration

### Environment Variables

- MEMGRAPH_URI (default: bolt://localhost:7687)
- MEMGRAPH_USER (optional)
- MEMGRAPH_PASSWORD (optional)
- AROUSAL_TAU_MS (default: 86400000)

## Data Model (Logical)

- Concept: name, valence, arousal_level, accessed_at
- Episode: summary, valence
- Relations: Concept->Concept with type in {is-a, part-of, evokes}
- Episode links: Concept->Episode using EVOKES

## Tools

### concept_upsert
Creates the concept if missing. Uses the concept string as-is.

Arguments:
- concept: string

Returns:
- concept_id: string
- created: boolean

### concept_update_affect
Adjusts valence by delta and conditionally updates arousal_level/accessed_at.

Arguments:
- concept: string
- valence_delta: number  # delta in [-1.0, 1.0]

Notes:
- valence is clamped to [-1, 1].
- arousal = arousal_level * exp(-(now - accessed_at) / tau)
- arousal_level/accessed_at update only if new arousal >= current arousal.
- update_affect uses new arousal_level = abs(valence_delta).

Returns:
- concept_id: string
- valence: number
- arousal: number
- accessed_at: number

### episode_add
Adds an episode summary and links it to concepts.

Arguments:
- summary: string
- concepts: string[]
- valence: number

Returns:
- episode_id: string
- linked_concepts: string[]
- valence: number

### relation_add
Adds a relation between two concepts.

Arguments:
- from: string
- to: string
- type: "is-a" | "part-of" | "evokes"

Notes:
- relation types are mapped to DB-safe labels (e.g., "is-a" -> "IS_A").
- tautologies (from == to) are rejected.

Returns:
- relation_id: string

### recall_query
Recalls propositions from seed concepts up to max_hop.

Arguments:
- seeds: string[]
- max_hop: number

Notes:
- propositions use a fixed text form, including episodes as
  "apple evokes <episode summary>".
- hop_decay is directional (forward: 0.5^(hop-1), reverse: 0.5^hop).
- for recall, new arousal_level = hop_decay and may update arousal if it raises.

Returns:
- propositions: Array<{ text: string, score: number, valence: number | null }>

## Usage Pattern (Example)

1) Create concepts
- concept_upsert: { concept: "apple" }
- concept_upsert: { concept: "fruit" }

2) Update affect
- concept_update_affect: { concept: "apple", valence_delta: 0.7 }

3) Add relation
- relation_add: { from: "apple", to: "fruit", type: "is-a" }

4) Add episode
- episode_add: { summary: "Bought apples at the market", concepts: ["apple"], valence: 0.2 }

5) Recall
- recall_query: { seeds: ["apple"], max_hop: 2 }
