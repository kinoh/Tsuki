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
- TZ (required, e.g. Asia/Tokyo)

## Data Model (Logical)

- Concept: name, valence, arousal_level, accessed_at
- Episode: name, summary, valence, arousal_level, accessed_at
- Relations: Concept->Concept with type in {is-a, part-of, evokes}, weight
- Episode links: Concept->Episode using EVOKES

Notes:
- MCP ensures a unique constraint on Concept(name) at startup; startup fails if existing data violates it.

## Tools

### concept_upsert
Creates the concept if missing. Uses the concept string as-is.

Arguments:
- concept: string

Notes:
- newly created concepts start with arousal_level = 0.5.

Returns:
- concept_id: string
- created: boolean

### update_affect
Adjusts valence by delta and conditionally updates arousal_level/accessed_at.

Arguments:
- target: string
- valence_delta: number  # delta in [-1.0, 1.0]

Notes:
- valence is clamped to [-1, 1].
- arousal = arousal_level * exp(-(now - accessed_at) / tau)
- arousal_level/accessed_at update only if new arousal >= current arousal.
- update_affect uses new arousal_level = abs(valence_delta).
- if target matches an Episode name, updates the episode; otherwise updates a concept (creating it if missing).

Returns:
- concept_id or episode_id: string
- valence: number
- arousal: number
- accessed_at: number

### episode_add
Adds an episode summary and links it to concepts.

Arguments:
- summary: string
- concepts: string[]

Notes:
- concepts created indirectly here start with arousal_level = 0.25.
- episodes are created with valence = 0.0 and arousal_level = 0.5.
- episode_id is "YYYYMMDD/<keyword>" using the first concept as keyword; duplicates add "-2", "-3", etc.
- episode_id is stored as Episode.name in Memgraph (for GUI visibility).
- episode_id is also de-duplicated against Concept names.

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
- concepts created indirectly here start with arousal_level = 0.25.
- relation weight is created at 0.25 and strengthened on repeated relation_add
  (weight = 1 - (1 - weight) * (1 - 0.2)).

Returns:
- from: string
- to: string
- type: string

### concept_search
Searches concepts by keyword (partial match) and fills with arousal-ranked concepts if needed.

Arguments:
- keywords: string[]
- limit: number (optional; default 50, max 200)

Notes:
- keywords are matched by partial name (case-insensitive).
- if matches are fewer than limit, fills remaining slots with arousal-ranked concepts.

Returns:
- concepts: string[]

### recall_query
Recalls propositions from seed concepts up to max_hop.

Arguments:
- seeds: string[]
- max_hop: number

Notes:
- propositions use a fixed text form, including episodes as
  "apple evokes <episode summary>".
- score = arousal * hop_decay * weight (for concept relations).
- hop_decay = 0.5^(hop-1); reverse relations apply a fixed 0.5 penalty.
- for recall, new arousal_level = hop_decay and may update arousal if it raises.

Returns:
- propositions: Array<{ text: string, score: number, valence: number | null }>

## Usage Pattern (Example)

1) Create concepts
- concept_upsert: { concept: "apple" }
- concept_upsert: { concept: "fruit" }

2) Update affect
- update_affect: { target: "apple", valence_delta: 0.7 }

3) Add relation
- relation_add: { from: "apple", to: "fruit", type: "is-a" }

4) Add episode
- episode_add: { summary: "Bought apples at the market", concepts: ["apple"] }

5) Recall
- recall_query: { seeds: ["apple"], max_hop: 2 }
