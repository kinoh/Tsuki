# Core Rust Web Debug UI (draft requirements)

## Goal
Provide a lightweight, local Web UI for developers to inspect and tune the
core-rust prompt flow. The UI focuses on visible input/output and event flow
rather than automated evaluation.

## Scope
- Target system: core-rust only
- Primary users: developers adjusting prompts
- Auth: not required (local/dev use)

## Primary User Outcomes
- See the flow of events from user input through submodules, decision, and
  final response.
- Edit any prompt (base, submodules, decision) and persist the changes locally.
- Execute a single module and view its output in isolation.
- Review historical runs and event logs.

## Data to Display
- Event stream (primary source of truth)
- User input events and tags
- Submodule outputs and decision outputs
- Tool calls and tool outputs (when present)
- Optional: raw LLM responses for debugging

## Prompt Editing
- All prompt types must be editable:
  - Base personality
  - Submodule instructions
  - Decision instructions
- Changes must be persisted to a single local markdown file under `core-rust/data`.
- The file should group prompts by section (base, decision, submodules).
- The UI should clearly show which prompt version is active for a run.

## Execution Controls
- Execution unit is a module (run one module on demand).
- The UI must allow running a single module without running the full pipeline.
- No success/failure criteria are enforced; outputs are reviewed by humans.

## History
- Event logs should be preserved across sessions.
- The UI should provide a simple run history view (event timeline preferred).
- Each run should be traceable to the prompt versions used.

## Non-Functional Requirements
- None beyond local developer usability.
- Performance and security constraints are out of scope for now.

## Open Questions
- How should module-only execution be wired into core-rust (new endpoint or
  debug-only mode)?
- Should prompt edits be hot-reloaded or applied on the next run only?
