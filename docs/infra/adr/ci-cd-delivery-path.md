---
date: 2026-02-28
---

# ADR: CI/CD Delivery Path as Definition of Done

## Context

Required runtime environment variables were introduced in application code without propagation to
deployment workflow and runtime environment wiring. Code compiled and passed tests while production
could not actually run.

## Decision

A change is not done until its delivery path is also updated and verified. When introducing or
changing required runtime env vars, the same change must update:
- runtime wiring (`compose.yaml`, container runtime env)
- CI/CD secret-to-env mapping (`.github/workflows/*`)
- operator-facing required env/secret documentation

Before completion, verify propagation with a repository-wide search.

## Rationale

Treating delivery wiring as part of implementation (not a follow-up) eliminates the gap between
"code compiles" and "production can actually run."
