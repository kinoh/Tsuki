# Conversation Recall Model

## Design Principle

Semantic recall of past conversation operates on two separate stores with distinct roles:

- **libSQL `events`** — canonical source of truth for all conversation history. Never displaced.
- **Memgraph vector index** — derived projection (`ConversationEvent`) for semantic search only.
  Keyed by `event_id`; rebuildable from libSQL at any time.

The vector index may be dropped and rebuilt without risking canonical history. Memgraph does not
own conversation bodies.

## Retrieval Flow

1. Latest user input is embedded and used to query the Memgraph vector index for semantic anchors.
2. Canonical event bodies are loaded from libSQL by `event_id`.
3. For each anchor, neighboring conversational events (user/assistant text only) within a
   configurable window (`surrounding_event_window`) are added from libSQL.
4. Anchors and neighbors are ranked, deduplicated, and sorted chronologically.
5. The result is injected into the decision context as `<recalled_event_history>` under
   `<supplemental_context>`.

Window expansion treats adjacency as a best-effort neighborhood, not strict turn reconstruction or
causal ordering.

## Indexing Scope

Only user and assistant text events are indexed for conversation recall. Internal control, debug,
decision, and submodule events are excluded.

## Failure Policy

- Canonical event persistence must succeed independently of projection updates.
- Projection failures are logged and repaired by backfill (`backfill-conversation-recall` CLI).
- Projection failures must not alter canonical history ownership or silently mutate the event store.

## Extension Guidance

- If recall needs to cover a new event type, add it to the indexing scope explicitly — do not
  relax the exclusion policy broadly.
- Recall is a Decision-time concern; do not add conversation recall to the Router path.
- If the Memgraph instance is replaced or reset, rebuild projections from libSQL before startup.
