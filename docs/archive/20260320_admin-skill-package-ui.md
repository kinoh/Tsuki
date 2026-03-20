# Admin Skill Package UI

## Overview
This change introduces a dedicated `/admin/skills` page for skill package management.
The page owns skill installation, package editing, and skill metadata inspection.
It replaces the older state-record-backed approach that mixed skill index concerns into the state editor.

## Problem Statement
The previous admin layout blurred responsibility boundaries:

- `state-records` exposed a skill indexing toggle even though state records are not skill packages.
- Skill package content lived behind the sandbox boundary, but there was no dedicated admin surface for it.
- The operator had to reason about skill metadata, sandbox files, and concept-graph indexing through the wrong screen.

That made the admin experience harder to understand and made the state editor depend on a skill-specific concept.

## Solution
Create a dedicated skill package admin page that treats the installed skill package as the primary object.

- `/admin/skills` renders the package editor and package overview.
- `/admin/skills/list` lists skill concepts from the concept graph.
- `GET /admin/skills/{key}` returns the installed package contents via `shell_exec__skill_read` plus the concept-graph skill metadata.
- `PUT /admin/skills/{key}` reuses the existing skill upsert path to install or update the package.

The page shows:

- `SKILL.md`
- auxiliary files
- summary
- trigger concepts
- required MCP tools
- concept-graph relations for the skill

## Design Decisions

### Keep state records pure
The state records page now only edits state-backed content. Skill indexing controls were removed.
This keeps the page aligned with its ownership boundary and avoids implying that state storage is the source of skill packages.

### Use the sandbox package as the skill source of truth
The skill page does not expose raw sandbox filesystem access.
Instead, it presents the installed skill package as a logical package view, backed by `shell_exec__skill_read`.

### Reuse the existing install path
The skill editor writes through the existing `skill_admin_service` path.
This keeps installation behavior consistent between the admin UI and the integration harness.

### Keep the concept graph as metadata, not the package store
The concept graph is used for listing and metadata inspection.
It is not used as the package store itself.

## Implementation Details

- Added a new admin route for `/admin/skills`.
- Added a skill list endpoint that reads skill concepts from the concept graph.
- Added a skill detail endpoint that merges:
  - concept graph metadata
  - `shell_exec__skill_read` package contents
  - auxiliary file bodies
- Updated the shared admin navigation to include the new Skills page.
- Removed the skill-index editor residue from the state records UI.

## Compatibility Impact
Breaking by default.

- The state records page no longer presents skill-index editing controls.
- Operators should use `/admin/skills` for skill package work.
- No compatibility layer was added between the two responsibilities.

## Future Considerations

- If package browsing needs richer tree navigation, it should stay within the skill page.
- If skill deletion is needed, it should be added as an explicit skill-package operation rather than through state records.
- If additional package metadata becomes necessary, it should be attached to the skill admin response, not copied into state records.
