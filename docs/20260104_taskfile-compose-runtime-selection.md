# Taskfile Compose Runtime Selection

## Overview
Introduce a local Compose override to avoid `runsc` on WSL and route Taskfile commands through a single `DOCKER_COMPOSE` variable.

## Problem Statement
The sandbox container uses `runtime: runsc`, but gVisor does not run on the local WSL environment. Developers need a safe local override without changing production configuration.

## Solution
Add `compose.dev.yaml` with `runtime: runc` for the `sandbox` service and switch Taskfile commands to use a `DOCKER_COMPOSE` variable that selects the override when `DOCKER_HOST` is unset.

## Design Decisions
- Use `DOCKER_HOST` presence as the signal for local vs. remote usage.
- Keep production `compose.yaml` unchanged and isolate local changes in `compose.dev.yaml`.
- Route all Taskfile Compose operations through a single variable for consistency.

## Implementation Details
- `Taskfile.yaml` now defines `DOCKER_COMPOSE` with a shell conditional:
  - Empty `DOCKER_HOST` → `docker compose -f compose.yaml -f compose.dev.yaml`
  - Set `DOCKER_HOST` → `docker compose`
- All Taskfile Compose commands now reference `{{.DOCKER_COMPOSE}}`.
- Compose commands are wrapped in YAML single-quoted strings to avoid `{{...}}` being misread as a mapping.
- `compose.dev.yaml` overrides only the sandbox runtime to `runc`.

## Future Considerations
- Replace `DOCKER_HOST` detection with a dedicated flag if local/remote usage patterns evolve.
