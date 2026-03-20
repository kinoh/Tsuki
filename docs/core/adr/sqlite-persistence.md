---
date: 2026-02-01
---

# ADR: SQLite as Single Persistence Layer

## Context

Events, internal state, and dynamic modules all need to survive restarts and support future HTTP
APIs. Multiple storage backends would complicate operations.

## Decision

Use SQLite (via libSQL) as the single persistence layer for:
- Event stream (`events`)
- Internal state (`state_records`)
- Module registry (`modules`)

Flexible fields (`related_keys`, `metadata`) are stored as JSON strings.
Local file by default; remote replica when `TURSO_DATABASE_URL` is set.

## Rationale

Unified storage simplifies operations and future API queries. libSQL provides SQLite compatibility
with optional remote replication. JSON strings preserve schema flexibility without complex
migrations.

## Compatibility Impact

breaking-by-default (no compatibility layer)
