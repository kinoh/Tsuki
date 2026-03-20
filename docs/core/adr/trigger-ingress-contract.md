---
date: 2026-03-01
---

# ADR: Generic Trigger Ingress Contract

## Context

`/triggers` endpoint used self-improvement-specific parameters (`target`, `reason`) despite a
generic name. This silently coupled a domain-neutral ingress to a single domain behavior and forced
self-improvement vocabulary onto all trigger producers including the scheduler.

## Decision

Trigger ingress contract:
- HTTP: `POST /triggers` with `{ "event": string, "payload": object }`
- WebSocket: `{ "type": "trigger", "event": string, "payload": object }`

Runtime behavior:
- Ingress emits exactly one event with `meta.tags = [event]` and `payload = payload`.
- `event` is the only required routing field.
- `payload` is untyped at ingress; schema ownership belongs to each consuming domain.

Self-improvement worker listens to `self_improvement.run` explicitly, not to a fixed ingress tag.

## Rationale

`event` as the single routing field avoids redundant `domain`+`action` splits — event selection is
already the dispatch boundary. Untyped payload keeps ingress generic. Fail-fast on old fields
aligns with the breaking-by-default policy.

## Compatibility Impact

breaking-by-default (no compatibility layer)
