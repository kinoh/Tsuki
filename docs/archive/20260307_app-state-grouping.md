# AppState Grouping

## Overview
This document records the first AppState cleanup step for `core-rust`.
The goal is to stop treating `AppState` as an unstructured bag of services, config fragments, and raw strings.

Compatibility Impact: Internal only. Public HTTP and WebSocket contracts are unchanged.

## Problem Statement
`AppState` lived in `server_app.rs` and mixed unrelated concerns:

- infrastructure handles such as `Db`, `EventStore`, broadcast sender, MCP registry, and concept graph access
- runtime orchestration state such as `Modules` and submodule saturation levels
- config fragments such as `limits`, `router`, `input`, and `tts`
- prompt file state and resolved prompt strings
- auth values stored as unrelated raw `String` fields

That shape made call sites hard to read because field names did not communicate responsibility boundaries.
It also made prompt and auth values look like arbitrary strings instead of domain-specific state.

## Solution
Move `AppState` into a dedicated `app_state.rs` module and group fields by role:

- `AppServices`
- `AuthState`
- `AppConfigState`
- `PromptState`
- `ResolvedPrompts`
- `RuntimeState`
- `AppMetadata`

Representative access patterns now read as:

- `state.services.db`
- `state.auth.web_auth_token`
- `state.config.router`
- `state.prompts.resolved.router_instructions`
- `state.runtime.modules`
- `state.metadata.api_versions`

## Design Decisions
### Group by responsibility, not by source file
The split is based on domain meaning rather than where a value happened to be created.
For example, prompt instructions are grouped under `PromptState` even though they are assembled in `server_app.rs`.

### Keep `Modules` under runtime state
`Modules` is still shared broadly, so this change does not yet split orchestration dependencies.
It is moved under `RuntimeState` to make that coupling explicit without changing runtime behavior.

### Store resolved prompt text as prompt state
`router_instructions` and `decision_instructions` were previously loose `String` fields.
They are now part of `ResolvedPrompts`, which makes it obvious that they come from prompt loading and validation rather than ad-hoc string assembly.

### Group auth strings immediately
`WEB_AUTH_TOKEN`, admin password, and password fingerprint remain `String` values, but they now live under `AuthState`.
This is the minimum step that gives those values a clear meaning at use sites without redesigning auth flow in the same change.

### Drop unused state from `AppState`
`state_store` is needed when constructing module tooling, but it was not read from `AppState` afterward.
It was removed from stored state to keep the container honest.

## Future Considerations
- Move request and response DTOs out of `server_app.rs` so `application/*` does not depend on server-owned types.
- Revisit whether `Modules` should remain in shared state or be passed as explicit orchestration dependencies.
- Consider separating mutable prompt overrides from immutable resolved prompt defaults if prompt editing logic keeps growing.
