# Debug UI Style Unification (Dark Palette, Token-Based)

## Date
- 2026-02-15

## Context
The user requested a unified look across existing debug/monitor/concept-graph UIs, without introducing theme switching.
The goal also includes making future UI additions easy to align visually.

## Decision
Introduced shared style assets under `core-rust/static/styles/` and wired all debug pages to consume them.

## Why
- Existing pages had duplicated and diverged color definitions.
- A shared token layer provides one change point for color adjustments.
- New pages can adopt consistent visuals by importing two CSS files.

## Implemented
- Added style route:
  - `GET /debug/styles/{name}`
  - allowed files: `ui-tokens.css`, `ui-base.css`
- Added shared CSS files:
  - `core-rust/static/styles/ui-tokens.css`
  - `core-rust/static/styles/ui-base.css`
- Updated pages to load shared CSS:
  - `core-rust/static/debug_ui.html`
  - `core-rust/static/monitor_ui.html`
  - `core-rust/static/concept_graph_ui.html`
- Aligned concept graph page to the same dark palette tokens used by existing debug/monitor surfaces.

## Operating Rule
- New debug pages should import shared styles first and avoid introducing new hard-coded palette values unless a token is added.
