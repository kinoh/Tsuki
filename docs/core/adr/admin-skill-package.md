---
date: 2026-03-20
---

# ADR: Admin Skill Package Surface — Dedicated `/admin/skills` Page

## Context

The previous admin layout placed skill indexing controls inside the state records page. State
records are not skill packages; mixing the two blurred responsibility boundaries and made operators
reason about skill metadata through the wrong screen.

## Decision

- `/admin/skills` is the dedicated surface for skill package management (install, edit, inspect).
- State records page no longer presents skill-index editing controls.
- Skill list is sourced from the concept graph. Skill package content is sourced from the sandbox
  via `shell_exec__skill_read`. Installation writes through `skill_admin_service`.
- The concept graph is used for listing and metadata inspection only — not as the package store.

## Rationale

Each admin screen should own one responsibility. Skill packages live in the sandbox; concept graph
holds metadata. Surfacing both through a dedicated page (not the state editor) keeps ownership
clear and the installation path consistent between admin UI and the integration harness.

## Compatibility Impact

breaking-by-default — state records page no longer provides skill-index editing controls
