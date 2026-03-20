---
date: 2026-02-23
---

# ADR: Fail Fast on Invalid Prompt Source

## Context

When prompt loading failed, runtime silently fell back to config defaults (`unwrap_or_default()`).
This made prompt source ambiguous in integration and debug runs, causing unexpected style and
instruction behavior that was hard to diagnose.

## Decision

- Prompt loading fails hard at startup when `prompts.path` is invalid, missing, or fails
  validation.
- No silent fallback to defaults.
- All prompt sections required by the runtime (including `# Self Improvement`) must be present and
  non-empty in `prompts.md`; startup fails if any are missing.

## Rationale

Prompt source must be explicit and trustworthy. Silent fallback breaks reproducibility and hides
misconfiguration. Failing at startup surfaces invalid prompt files immediately.

## Compatibility Impact

breaking-by-default (no compatibility layer)
