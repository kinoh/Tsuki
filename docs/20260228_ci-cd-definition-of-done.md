# CI/CD Delivery Path in Definition of Done

## Overview
Updated agent guidance so runtime changes are considered incomplete unless CI/CD and deployment wiring are updated in the same change.

## Problem Statement
- Required runtime environment variables were introduced in application code without complete propagation to deployment workflow and runtime environment wiring.
- This created a gap between "code compiles" and "production can actually run".

## Solution
- Added a "Definition of Done (delivery path)" rule in `AGENTS.md`.
- The rule makes delivery-path completeness mandatory for required runtime env changes.

## Design Decisions
- Treat delivery wiring as part of implementation, not a follow-up task.
- Require explicit updates for:
  - runtime wiring (`compose.yaml` and container env)
  - CI/CD secret-to-env mapping (`.github/workflows/*`)
  - operator-facing required env/secret documentation
- Require a final propagation check with repository search (`rg`) before completion.

## Compatibility Impact
- No runtime compatibility impact.
- Process contract changed: task completion now includes CI/CD propagation verification.
