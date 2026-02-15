# Core-Rust AGENTS Guidance Baseline

## Overview
This note records why `core-rust/AGENTS.md` was introduced now and what it intentionally fixes versus leaves as WIP.

## Problem
Design and implementation intent were spread across many dated notes. Contributors could read current code and still miss responsibility boundaries or accidentally treat unsettled areas as finalized policy.

## Decision
- Added `core-rust/AGENTS.md` as a contributor-facing baseline.
- Consolidated stable guidance around:
  - router/application/decision/submodule responsibility split,
  - router-first runtime invariants,
  - transport-vs-application layering,
  - config policy and debug-event handling.
- Marked unsettled areas explicitly as WIP:
  - Event Log vs Work Log UI emphasis,
  - prompt-level persistence policy over libSQL-backed state,
  - self-improvement redesign direction,
  - test strategy maturity.

## Why
- Responsibility boundaries are the highest-value guardrails for near-term work.
- WIP labeling prevents false certainty and reduces accidental overdesign.
- The file is intentionally pragmatic: stable rules are enforceable, unsettled rules are transparent.

## Constraints Applied
- Avoided treating UI terms (`Work Log`) as runtime semantics.
- Avoided overcommitting to self-improvement and persistence conventions that are still being reconsidered.
- Kept references to existing decision docs for traceability.

## Follow-up
- Promote WIP sections to stable only after explicit decision notes are added.
- If conflicting docs are found, reconcile in a new dated doc and update `core-rust/AGENTS.md`.

## Additional Update (same day)
- Added a dedicated `Terms` section in `core-rust/AGENTS.md`.
- Each project-specific term now includes:
  - concise definition,
  - ownership boundary,
  - misuse guard (`Must not`).

### Why this addition
- Responsibility statements alone were not enough; ambiguous terms still allowed divergent interpretation.
- Explicit term contracts reduce review ambiguity and make boundary checks more mechanical.
