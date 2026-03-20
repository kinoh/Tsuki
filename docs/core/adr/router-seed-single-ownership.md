---
date: 2026-02-24
---

# ADR: Router Seed Selection — Single Ownership

## Context

A secondary relevance-scoring filter was introduced after router LLM seed selection to block
non-conversational seeds. This conflicted with the intended router model: the router should
tolerate ambiguity and decide seeds directly. Speed is prioritized in the router stage; downstream
modules must not reinterpret router intent.

## Decision

- Router LLM output is the sole owner of seed selection.
- No secondary relevance judgement is applied after router output.
- When router returns no seeds, no automatic arousal-ranked backfill is applied.
- Seed selection rules are owned by the router prompt config template, not by Rust source code.

## Rationale

A single ownership point makes router behavior predictable and auditable. Secondary filtering
creates an implicit second decision layer that is harder to observe and override.

## Compatibility Impact

breaking-by-default (no compatibility layer)
