# Tester Prompt Naturalness Adjustment for Submodule Scenario

## Decision
Adjusted tester-side prompting so scenario progression is controlled by intent checkpoints instead of fixed sentence templates.

## Why
`dialog_naturalness` was being depressed by mechanical tester utterances that mirrored scenario checklist text too literally. This made conversation flow look scripted rather than naturally conversational.

## Changes
- Updated global tester prompt at `core-rust/tests/integration/prompts/tester.md`:
  - explicitly discourage verbatim checklist copying
  - require natural Japanese rephrasing and light conversational connectors
  - discourage checklist-like wording
- Updated `core-rust/tests/integration/scenarios/submodule.yaml`:
  - replaced fixed turn text with semantic intent checkpoints
  - kept strict turn order and one-sentence constraint

## Scope
No API/interface change. Prompt-level behavior tuning only.
