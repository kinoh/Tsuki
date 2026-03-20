# Prompt State Base Instructions

## Overview
This document records the removal of duplicated base prompt text from `ModuleRuntime`.

Compatibility Impact: Internal only. Prompt behavior is unchanged.

## Problem Statement
The resolved base prompt text existed in two places:

- `PromptState.resolved.base_instructions`
- `ModuleRuntime.base_instructions`

That duplication made it unclear which one was authoritative and increased the chance of future drift.

## Solution
Keep resolved base prompt text only under `PromptState`.
`ModuleRuntime` now contains runtime execution settings and tool wiring only.

Call sites that previously read `modules.runtime.base_instructions` now read `state.prompts.resolved.base_instructions`.

## Design Decisions
### Treat prompt text as prompt state, not module runtime
Base prompt text comes from prompt loading and validation.
It is not a runtime execution primitive in the same way as model name, tool handler, or tool limits.

### Keep the refactor narrow
This change does not redesign prompt override flow.
It only removes duplicated ownership so later prompt-state cleanup has a single default source.

## Future Considerations
- Revisit whether resolved prompt defaults and mutable prompt overrides should remain in the same state container.
