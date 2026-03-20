---
date: 2026-03-04
---

# ADR: Runtime Configuration Boundary

## Context

`core-rust` mixed configuration sources: `config.toml`, environment variables, `DATA_DIR` fallback
logic, and env-tunable runtime parameters. This created unclear ownership and implicit fallback
chains that were hard to reason about.

## Decision

- **Secrets** live in environment variables only: `WEB_AUTH_TOKEN`, `ADMIN_AUTH_PASSWORD`,
  `OPENAI_API_KEY`, `MEMGRAPH_PASSWORD`, `TURSO_AUTH_TOKEN`.
- **Non-secret runtime values** live in `config.toml` with no env fallback:
  `[prompts].path`, `[concept_graph].*`, `[tts].*`, etc.
- `config.toml` is required at startup. Missing or malformed files fail fast.
- `DATA_DIR`-derived fallbacks for prompt path are removed.
- Runtime-tunable env overrides (e.g. `CONCEPT_VECTOR_*`) are removed; those values are fixed
  constants in code.

## Rationale

Explicit config ownership is simpler and more predictable than mixed source precedence. Prompt path
must be a named explicit contract, not derived from a directory convention. Vector search parameters
should be deterministic and code-owned, not runtime-tunable via env.

## Compatibility Impact

breaking-by-default (no compatibility layer)
