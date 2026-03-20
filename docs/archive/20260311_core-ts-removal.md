# Legacy TypeScript Core Removal

## Overview

This change removes the legacy TypeScript `core/` implementation from the repository after the production/backend cutover to `core-rust` had already completed.

Compatibility Impact: breaking-by-default

## Problem Statement

The repository still contained the old `core/` tree, its Docker image, and several current entrypoints that referenced it. That created two concrete problems:

- Current documentation described the wrong backend as primary.
- Task and tooling paths still depended on a directory that was no longer part of the active runtime model.

Keeping the legacy tree as "insurance" was not justified because repository history already preserves the removed implementation.

## Solution

- Delete the tracked `core/` source tree.
- Delete the unused `docker/core/` image definition.
- Remove current Taskfile and README references that depended on `core/`.
- Keep operational service naming in Compose unchanged for now (`core`, `core-data`) to avoid mixing code retirement with deployment-label renames.

## Design Decisions

### Repository history is the rollback path

The deleted TypeScript implementation remains available through git history. The active repository surface should represent the current architecture, not an already-retired fallback.

### Historical docs remain historical

Files under `docs/` are historical design records. They may continue to mention `core/` when they describe past decisions. Only current entrypoints and operator-facing references were updated.

### Service rename is out of scope

The Compose service name `core` now means "the main runtime service", not "the TypeScript implementation". Renaming that service would expand the blast radius into operations, deployment scripts, and runbooks without being required for code retirement.

## Implementation Details

- Root documentation now points to `core-rust/` as the backend.
- Root Taskfiles no longer depend on `core/.env`, `core/bin`, or `core/Taskfile.yaml`.
- The `core-rust` README no longer points to deleted Node-based helper scripts.

## Future Considerations

- Rename runtime service and backup labels away from `core` only if operational vocabulary cleanup becomes a dedicated task.
- Remove stale deployment secrets and docs only when they are confirmed to be unrelated to current runtime behavior.
