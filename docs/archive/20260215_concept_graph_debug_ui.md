# Concept Graph Debug UI (Read-only MVP)

## Date
- 2026-02-15

## Context
The user requested an initial implementation to validate the UX direction for a Concept Graph management screen in `core-rust`, focusing on:
1. Graph health and summary counts
2. Read-only concept search and inspection
3. Router usage timeline for concept graph queries

## Decision
Implemented a read-only debug surface first, instead of direct mutation controls.

## Why
- Current runtime behavior relies on concept activation visibility (`concept_graph.query`) to understand routing quality.
- Operational risk is lower with observation-first rollout.
- It enables immediate debugging value without introducing accidental graph edits.

## Implemented Scope
- New debug routes in `core-rust/src/main.rs`:
  - `GET /debug/concept-graph/ui`
  - `GET /debug/concept-graph/health`
  - `GET /debug/concept-graph/stats`
  - `GET /debug/concept-graph/concepts`
  - `GET /debug/concept-graph/concepts/{name}`
  - `GET /debug/concept-graph/queries`
- New UI file:
  - `core-rust/static/concept_graph_ui.html`
- New read-only debug interface in concept graph store:
  - Health check
  - Aggregate counts
  - Concept search with arousal-aware ordering
  - Concept detail (state, relations, episodes)

## Non-Goals (for this change)
- No create/update/delete operations for concepts/relations/episodes.
- No rollback, audit trail mutation UI, or batch maintenance actions.

## Additional Decision (Tab Switch UX)
- Updated the list/search panel to be mode-switchable by clicking the top entity cards:
  - `Concepts`
  - `Episodes`
  - `Relations`
- Why:
  - The user needed a quick way to inspect each entity type from the same panel without changing pages.
  - Reusing one panel keeps interaction cost low during debugging sessions.
- Supporting APIs were added for read-only episode/relation retrieval:
  - `GET /debug/concept-graph/episodes`
  - `GET /debug/concept-graph/episodes/{name}`
  - `GET /debug/concept-graph/relations`

## Notes
- Timeline endpoint currently derives query history from persisted event log entries tagged `concept_graph.query`.
- The UI polls health/counts/timeline periodically to stay useful during live debugging.
