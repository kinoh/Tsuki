# Prompt Default Resolution

## Overview
This document records the consolidation of prompt default resolution logic into `PromptState`.

Compatibility Impact: Internal only. Effective prompt text is unchanged.

## Problem Statement
Multiple modules repeated the same fallback pattern:

- read override from `PromptOverrides`
- otherwise fall back to `state.prompts.resolved.*`

That duplicated prompt ownership knowledge across router, execution, approval, and server code.

## Solution
Add small helper methods on `PromptState`:

- `base_or_default`
- `router_or_default`
- `decision_or_default`

Call sites now ask `PromptState` for the effective default instead of re-encoding fallback rules locally.

## Design Decisions
### Keep fallback rules next to prompt state
The state object already owns both mutable overrides and resolved defaults.
It should also own the policy for choosing between them.

### Avoid a broader prompt service for now
This step is intentionally small.
It reduces duplication without introducing another service layer before the prompt boundary is fully stabilized.

## Future Considerations
- If prompt access patterns continue to grow, move all effective prompt composition behind a dedicated prompt resolver module.
