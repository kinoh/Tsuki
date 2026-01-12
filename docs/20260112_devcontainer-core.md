# Core Devcontainer Refresh

## Overview
Switch the core devcontainer to a lightweight compose override that mounts the full repository and relies on host `core/node_modules`, while keeping network dependencies aligned with the main compose stack.

## Problem Statement
- The existing devcontainer builds a custom image, which is slow to rebuild during dependency churn.
- Local endpoints are currently tied to `isProduction` branching, which makes container-based dev environments awkward.
- The devcontainer should mirror the compose network (memgraph/sandbox) without changing how developers install dependencies on the host.

## Solution
- Update `.devcontainer` to use `compose.yaml` with a minimal override for the `core` service.
- Mount the full repo into the container and use host `core/node_modules` directly.
- Move memgraph and sandbox MCP endpoints behind explicit config values sourced from environment variables.

## Design Decisions
- Use the stock `node:22-bullseye` image and `sleep infinity` to avoid rebuilds and keep the container focused on mounts/networking.
- Keep compose networking (no host network) so service discovery uses `memgraph` and `sandbox` hostnames.
- Introduce `MEMGRAPH_URI` and `SANDBOX_MCP_URL` as explicit overrides to remove `isProduction` branching for endpoints.
- Install `pnpm` in the user prefix via `postCreateCommand` to avoid image rebuilds and avoid version pinning.

## Implementation Details
- `.devcontainer/compose.yaml` overrides `core` with:
  - `working_dir` set to `/workspaces/tsuki/core`.
  - repo mount `.:/workspaces/tsuki` (compose paths resolve from repo root, so `..` was incorrect).
  - `core-data` and `sandbox-data` mounts preserved.
  - environment overrides for `NODE_ENV`, `MEMGRAPH_URI`, and `SANDBOX_MCP_URL`.
- `.devcontainer/devcontainer.json` installs `pnpm` on container creation via `postCreateCommand`, sets `NPM_CONFIG_PREFIX=/home/node/.npm-global`, and defines an explicit `PATH` that keeps `/usr/local/bin` while adding `/home/node/.npm-global/bin` to avoid losing the default entrypoint.
- `ConfigService` now exposes `memgraphUri` and `sandboxMcpUrl`, used by the MCP client configuration.

## Future Considerations
- If host OS differs from container (e.g. macOS), native dependencies in `core/node_modules` may need a container-side install instead of host reuse.
