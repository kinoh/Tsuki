# Conversation Event Recall Index

Compatibility Impact: breaking-by-default (no compatibility layer)

## Overview
This document records the decision to add semantic recall for past conversation events in `core-rust`.

The feature reuses Memgraph only as a vector index for searchable conversation-event projections while keeping libSQL `events` as the canonical event store.

## Problem Statement
- Decision currently sees only a short recent event window.
- Older but semantically relevant past dialogue cannot be recalled unless it remains inside that recent window.
- The repository already treats `/events` and libSQL `events` as the canonical conversation history contract, so moving history ownership into Memgraph would conflict with the existing replacement policy.

## Decision

### Canonical storage
- `libSQL.events` remains the only source of truth for conversation history.
- No conversation body text is moved out of `events`.

### Vector index role
- Memgraph stores only a derived projection for searchable conversation events.
- The projection uses `:ConversationEvent { event_id, ts, source, embedding }`.
- Projection rows are keyed by `event_id` and are rebuildable from libSQL.

### Responsibility boundaries
- `application/event_service`
  - persists canonical events to libSQL
  - triggers best-effort projection updates for conversation recall
- `conversation_recall_store`
  - defines the projection/update/search contract for conversation recall
- `activation_concept_graph`
  - remains the Memgraph runtime owner
  - implements the conversation recall projection/search contract on the same Memgraph connection and embedding runtime
- `application/conversation_recall_service`
  - queries semantic matches for the latest user input
  - loads canonical event bodies from libSQL by `event_id`
  - applies final ranking/formatting for decision context
- `application/execution_service`
  - injects recalled conversation lines into decision context
  - keeps router responsibilities unchanged

### Retrieval scope
- Only conversational user inputs and assistant responses are indexed.
- Internal control/debug/decision/submodule events are excluded from conversation recall indexing.
- Decision receives recalled history as a separate context block from `recent_event_history`.
- Router does not use conversation-event recall in this change.

## Rationale
- Keeps source-of-truth ownership aligned with the existing `/events` contract.
- Allows the vector index to be dropped and rebuilt without risking canonical history corruption.
- Avoids mixing concept activation semantics with conversation-history ownership.
- Avoids dependence on strict event adjacency or synthetic turn reconstruction, which is incompatible with the event-stream policy.

## Failure Policy
- Canonical event persistence must succeed independently of the projection update.
- Projection update failures are logged explicitly and repaired by backfill.
- No fallback path changes runtime ownership or silently mutates the canonical history model.

## Implementation Details
- Add `conversation_recall` config for:
  - enable/disable
  - result limit
  - semantic weight
  - recency weight
  - recency decay tau
- Add `backfill-conversation-recall` CLI command to rebuild projections from canonical events.
- Add `recalled_event_history` placeholder to the decision context template.

## Rejected Alternatives
- Store conversation embeddings directly on libSQL `events`
  - rejected because canonical event storage and derived vector-index concerns would be mixed in one table contract
- Store conversation bodies primarily in Memgraph
  - rejected because it would duplicate or displace canonical history ownership from libSQL
- Add conversation recall to router
  - rejected because router already owns concept activation and recall-seed selection, while this feature is for decision-time dialogue recall
