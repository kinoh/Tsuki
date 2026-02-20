# Router Prompt Configuration and Override Order

## Context
- Router prompt behavior diverged from other modules.
- `prompts.md` was loaded, but router instruction text still came from a hardcoded string appended to `base_personality`.
- This caused prompt inconsistency and made prompt maintenance error-prone.

## Decision
- Add `llm.router_instructions` to `core-rust/config.toml`.
- Extend `prompts.md` schema with a top-level `# Router` section.
- Use the same override model as other modules:
  - `base`: `prompts.base` -> fallback `config.llm.base_personality`
  - `router`: `prompts.router` -> fallback `config.llm.router_instructions`
- Router runtime instructions are composed as `base + router`.

## Why
- Router prompt text must be configurable in config first, per team direction.
- Prompt behavior should be consistent across router/decision/submodules.
- A shared override order prevents hidden drift between runtime components.

## Additional Alignment
- `/debug/prompts` payload now includes `router`.
- Improve flow accepts `target=router` so router prompt updates can be applied and persisted through the same path as other prompt targets.
