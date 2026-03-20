# Taskfile Local-Only Tasks

## Overview
Move local-only maintenance tasks into a dedicated Taskfile and remove an unused model download task.

## Problem Statement
Local-only tasks (`memgraph/local-clean`, `local-reset`) are dangerous to run against remote environments. The shared Taskfile also contained an unused model download task.

## Solution
Create `Taskfile.local.yaml` for local-only tasks, force local Docker usage, and remove `download_model` from the shared Taskfile.

## Design Decisions
- Keep shared tasks in `Taskfile.yaml` and isolate dangerous local maintenance in `Taskfile.local.yaml`.
- Force `DOCKER_HOST` to empty in `Taskfile.local.yaml` to prevent remote execution.
- Remove `download_model` since it is unused and environment-specific.

## Implementation Details
- `Taskfile.local.yaml` defines `memgraph/local-clean` and `local-reset`.
- `Taskfile.yaml` no longer includes local-only tasks or the model download task.

## Future Considerations
- Add explicit safety checks before destructive local tasks if usage grows.
