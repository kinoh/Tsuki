---
date: 2026-04-06
---

# ADR: Separate Turn Experimentation From Events Monitoring

## Context

The admin `Events` screen is the monitoring surface for raw event flow. Turn replay and prompt A/B
comparison require a different interaction model: select an assistant turn, override prompts, run
experiments, and compare outputs. Adding those controls directly into `Events` mixed observation
with intervention and made the event detail pane carry prompt-domain responsibilities.

The existing `Prompts` screen already has dense editing state and is not a good place to absorb
turn-scoped experimentation.

## Decision

- Keep `/admin/events` focused on event monitoring and detail inspection.
- Introduce `/admin/turns` as the turn-scoped experimentation workspace.
- Allow `Events` to link into `Turns` for a selected assistant response, but keep replay and A/B
  controls out of the monitoring surface.

## Rationale

Monitoring, prompt management, and turn experimentation are distinct operator tasks with different
state and failure modes. A dedicated `Turns` screen keeps each task legible and avoids coupling the
event monitor to prompt-loading and prompt-override UI behavior.

## Compatibility Impact

breaking-by-default (admin navigation changed; no compatibility layer)
