# Visible History Limit Semantics

Compatibility Impact: breaking-by-default (no compatibility layer)

## Overview
This document records the decision that `decision_history` and `submodule_history` must mean visible history counts, not raw fetched event counts.

## Problem Statement
- History assembly previously fetched the latest `N` events first and filtered debug/observability events afterward.
- As a result, `decision_history = 30` did not guarantee that the decision model actually received 30 visible events.
- The mismatch became worse as debug observability became more verbose.

## Decision
- `limits.decision_history` now means the number of non-debug, non-observability events visible to decision context.
- `limits.submodule_history` now means the number of non-debug, non-observability events visible to submodule context.
- History assembly must continue fetching older events until it has gathered the requested number of visible events or exhausted the stream.

## Rationale
- Config names should reflect the number of events actually shown to the model.
- A visibility-based contract is the only interpretation that stays stable when debug event volume changes.
- This keeps prompt-history quality aligned with operator expectations without changing router responsibilities.

## Implementation Notes
- History retrieval now pages backward through canonical events in descending order.
- Filtering still excludes debug and observability events before prompt formatting.
- The final visible history block remains chronological when rendered into prompts.
