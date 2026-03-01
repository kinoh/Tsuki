# Event Constructor Sealing for Core-Rust

## Overview
This decision seals direct event construction in `core-rust` and routes all event emission through event-contract constructors.

Compatibility Impact: `breaking-by-default (no compatibility layer)`.

## Problem
- Runtime code used `build_event(...)` directly across many modules.
- Contract drift could only be detected by review or CI checks.
- User feedback required this to be impossible by code structure, not only policy.

## Decision
- Keep `build_event` private inside `event` module.
- Introduce `event::contracts` as the only event construction surface.
- Add a private sealed field to `Event` so external struct-literal construction is impossible.
- Keep rehydration from DB explicit through `event::rehydrate_event(...)`.

## Why
- Enforces event emission through contract-level constructors at compile time.
- Removes the class of "accidental uncontracted event emission" without relying on lint/grep rules.
- Keeps runtime emission logic in application modules while centralizing envelope and tag construction.

## Implementation Notes
- Added: `core-rust/src/event/contracts.rs`
- Updated all former `build_event(...)` call sites to use contract constructors.
- `core-rust/src/event.rs` now owns:
  - private constructor (`build_event`)
  - sealed `Event`
  - explicit `rehydrate_event` path for storage loading
