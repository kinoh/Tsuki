# Core-Rust Runtime Config Boundary (No Compatibility Fallback)

## Overview
This document records the runtime configuration boundary update for `core-rust` after user feedback.

Compatibility Impact: breaking-by-default (explicitly no backward compatibility).

Supersedes:
- `docs/20260301_core-rust-prompts-path-config-priority.md`

## Problem Statement
`core-rust` mixed runtime configuration sources:
- `config.toml` for some runtime values
- environment variables for non-secret operational values
- `DATA_DIR` fallback logic for prompt file path
- runtime tuning for concept vector search exposed via `CONCEPT_VECTOR_*` env vars

This created unclear ownership for configuration and made runtime behavior depend on implicit fallbacks.

## Decision
- Keep secrets in environment variables only:
  - `WEB_AUTH_TOKEN`
  - `ADMIN_AUTH_PASSWORD`
  - `OPENAI_API_KEY`
  - `MEMGRAPH_PASSWORD`
  - `TURSO_AUTH_TOKEN` (when `db.remote_url` is used)
- Move non-secret runtime values to `config.toml` with no env fallback:
  - `[prompts].path` (required, explicit path)
  - `[concept_graph].memgraph_uri`
  - `[concept_graph].memgraph_user`
  - `[concept_graph].arousal_tau_ms`
  - `[tts].ja_accent_url`
  - `[tts].voicevox_url`
  - `[tts].voicevox_speaker`
  - `[tts].voicevox_timeout_ms`
- Remove `DATA_DIR` behavior from prompt path resolution.
- Remove `CONCEPT_VECTOR_*` runtime overrides; vector index/search parameters are fixed constants in code.
- Keep `GIT_HASH` out of runtime config.

## Why
- `core-rust` is still under development and not deployed as a compatibility target.
- Explicit config ownership is simpler and easier to reason about than mixed source precedence.
- Prompt path must be an explicit contract (`db.path`-style) instead of implicit directory-derived behavior.
- Concept vector index/search behavior should be deterministic and code-owned, not runtime-tunable via env.

## Implementation Notes
- `core-rust/src/config.rs`
  - Added required sections: `concept_graph`, `tts`.
  - Changed `prompts.path` to required string.
- `core-rust/src/server_app.rs`
  - Added startup config validation for required non-secret fields.
  - Replaced env reads for concept graph and TTS settings with config values.
  - Removed `DATA_DIR` fallback for prompt path.
- `core-rust/src/activation_concept_graph.rs`
  - Removed `CONCEPT_VECTOR_*` env-based overrides.
  - Kept fixed constants for vector index/search tuning.
- `core-rust/config.toml`, `core-rust/config.prod.toml`
  - Added required sections for prompts, concept graph, and TTS.
- `compose.yaml`
  - Removed unused `DATA_DIR` and `MEMGRAPH_URI` container env wiring for core-rust runtime.
