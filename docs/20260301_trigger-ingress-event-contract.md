# Trigger Ingress Event Contract

## Overview
The `/triggers` endpoint and WebSocket trigger ingress were changed from self-improvement-specific parameters (`target`, `reason`) to a generic event contract (`event`, `payload`).

## Problem Statement
A generic endpoint name (`/triggers`) was coupled to a single domain behavior (self-improvement) and silently emitted a fixed tag (`self_improvement.triggered`).

This created two design contradictions:
- Ingress naming looked domain-neutral but behavior was domain-specific.
- Trigger request shape forced self-improvement vocabulary onto all external trigger producers (including schedulers).

## Solution
The ingress contract is now:
- HTTP: `POST /triggers` with `{ "event": string, "payload": object }`
- WebSocket: `{ "type": "trigger", "event": string, "payload": object }`

Runtime behavior:
- Ingress emits exactly one event with `meta.tags = [event]` and `payload = payload`.
- Self-improvement worker no longer listens to a fixed ingress-only tag (`self_improvement.triggered`), and instead consumes `self_improvement.run` explicitly.

## Design Decisions
- `event` is the only required routing field.
  - Why: splitting into `domain` and `action` was redundant because event selection is already the dispatch boundary.
- `payload` remains untyped at ingress boundary.
  - Why: keep ingress generic and move schema ownership to each consuming domain.
- No compatibility fallback for old trigger fields.
  - Why: fail-fast aligns with `core-rust` breaking-by-default policy and avoids hidden behavior.

## Implementation Details
- Updated `core-rust/src/application/trigger_ingress_api.rs` to validate non-empty `event` and emit tag from request.
- Updated `core-rust/src/application/debug_service.rs` WebSocket parser for trigger messages.
- Updated self-improvement trigger consumer in `core-rust/src/application/improve_service.rs` to subscribe to `self_improvement.run`.
- Updated integration harness/scenario docs to emit generic trigger payloads.

## Supersedes
This document clarifies and partially supersedes trigger-ingress parts of:
- `docs/20260222_improvements-api-operational-endpoints.md`

## Compatibility Impact
breaking-by-default (no compatibility layer)
