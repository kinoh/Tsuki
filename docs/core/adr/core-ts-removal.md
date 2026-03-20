---
date: 2026-03-11
---

# ADR: Legacy TypeScript Core Removal and Rename

## Context

The repository contained the old `core/` (TypeScript/Mastra) implementation alongside
`core-rust/` after production cutover to `core-rust` was complete (2026-02-28).
This created two problems:

- Documentation described the wrong backend as primary.
- Task and tooling paths depended on a directory no longer part of the active runtime.

## Decision

- Delete the tracked `core/` source tree (2026-03-11).
- Remove Taskfile and README references that depended on `core/`.
- Keep Compose service name `core` unchanged — it now refers to the Rust runtime service.
- Rename `core-rust/` source directory to `core/` (post-removal).

## Rationale

Repository history is the rollback path; the active surface should represent current architecture.
Renaming the Compose service would expand blast radius into operations, runbooks, and deployment
scripts without benefit.

## Naming Timeline

| Period | `core/` | `core-rust/` |
|---|---|---|
| Before 2026-03-11 | TypeScript implementation | Rust implementation |
| 2026-03-11 | deleted | Rust implementation (production) |
| After rename | Rust implementation (same as `core-rust/` was) | — |

Historical `docs/archive/` files that mention `core/` before 2026-03-11 refer to the TypeScript
implementation. Files mentioning `core-rust/` refer to what is now `core/`.

## Scope

- `docs/archive/` files are historical records and may continue to mention `core/` for past
  decisions. Only current entrypoints and operator-facing references were updated.
- Compose service rename is out of scope; address only if operational vocabulary cleanup becomes
  a dedicated task.

## Compatibility Impact

breaking-by-default (no compatibility layer)
