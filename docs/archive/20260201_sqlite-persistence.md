# Decision: Persist core data in libSQL (Turso)

## Context
We want events, internal state, and dynamic modules to survive restarts and support future HTTP APIs.
Since internal state already needs a database, keeping other stores in SQLite/libSQL avoids split storage.

## Decision
- Use libSQL (Turso) as the single persistence layer for:
  - Event stream (`events`)
  - Internal state (`state_records`)
  - Module registry (`modules`)
- Store `related_keys` and `metadata` as JSON strings.
- Use a local file by default and remote replica when `TURSO_DATABASE_URL` is set.

## Rationale
- Unified storage simplifies operations and future API queries.
- libSQL provides SQLite compatibility with optional remote replication.
- JSON strings preserve flexible structures without complex schema changes.

## Consequences
- Database access becomes part of core runtime initialization.
- JSON-based fields rely on JSON1-style querying for advanced filtering.
