# Conversation Recall Windowed Neighborhood

Compatibility Impact: breaking-by-default (no compatibility layer)

## Overview
This document records the decision to change decision-time conversation recall from "direct hit lines only" to "semantic hit anchors plus nearby conversational events".

## Problem Statement
- A semantic hit can land on the past user question while missing the assistant reply that the user actually wants recalled.
- The previous `limit` parameter controlled only the number of vector hits, not the amount of surrounding conversational context.
- The kernel-wording integration scenario showed this failure mode directly: the past kernel question was recalled, but the assistant-side explanation line was absent from `recalled_event_history`.

## Decision
- Replace the single `conversation_recall.limit` control with two explicit parameters:
  - `conversation_recall.top_k_hits`
  - `conversation_recall.surrounding_event_window`
- `top_k_hits` controls how many semantic anchor events are kept after ranking.
- `surrounding_event_window` controls how many conversational events before and after each anchor are added from canonical libSQL history.
- Window expansion must filter to conversational `user` / `assistant` text events only.
- Window expansion must not treat adjacency as strict causality or synthetic turn reconstruction.
  - It is only a best-effort neighboring conversation slice around a semantic anchor.

## Rationale
- The user-facing need is often the assistant wording near a recalled question, not the question line alone.
- Splitting anchor count from neighborhood width makes recall behavior tunable without conflating two different retrieval concerns.
- Expanding from canonical libSQL history preserves the design that Memgraph is only the vector index and not the owner of conversation bodies.

## Implementation Notes
- Direct semantic hits are still scored by semantic similarity and recency.
- Neighboring conversational events inherit the best anchor score of any window that includes them.
- The formatted `recalled_event_history` block is sorted chronologically after deduplication so the decision model sees a readable conversation slice.
