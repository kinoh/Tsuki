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
- See the execution flow from user input through submodules, decision, and
  final response.
- Edit any prompt (base, submodules, decision) and persist the changes locally.
- Execute a single module and view its output in isolation.
- Review historical runs as a work log (run-level summaries).

## Data to Display
- Execution flow for the current run (primary UI focus)
- Work log entries (run-level summaries)
- Event details available on demand (backing data, not the primary view)
- Submodule outputs and decision outputs
- Tool calls and tool outputs (when present)

## Prompt Editing
- All prompt types must be editable:
  - Base personality
  - Submodule instructions
  - Decision instructions
- Changes must be persisted to a single local markdown file under `core-rust/data`.
- The file should group prompts by section (base, decision, submodules).
- The UI should clearly show which prompt version is active for a run.

### `prompts.md` Format
```
# Base
```text
<base personality>
```
# Decision
```text
<decision instructions>
```
# Submodules
## curiosity
```text
<submodule instructions>
```
```

## Execution Controls
- Execution unit is a module (run one module on demand).
- The UI must allow running a single module without running the full pipeline.
- Module-only runs must not execute other modules.
- No success/failure criteria are enforced; outputs are reviewed by humans.

## History
- Event logs should be preserved across sessions.
- Work logs are stored as debug-only events (not a separate data store).
- The UI should render work logs by filtering debug events.

## Implementation Approach (UI)
- Layout:
  - Left: work log (run-level history)
  - Center: execution flow visualization
  - Right: prompt editor + module run controls
- Normal mode flow: `User Input → Submodules → Decision → emit_user_reply`
- Module-only mode flow: `User Input → [Selected Module]` (others hidden/disabled)
- Work log entries summarize input, selected module(s), and outputs; events are
  available as expandable details.

## Implementation Approach (core-rust)
- Add debug-only endpoints:
  - `GET /debug/prompts` for current prompt values
  - `POST /debug/prompts` to persist and apply prompt edits
  - `POST /debug/modules/:name/run` to run a single module
  - `GET /debug/runs` (or `GET /debug/events?group=run`) as a derived work log view
- Prompt persistence:
  - Store all prompts in `core-rust/data/prompts.md`
  - Load at startup; apply edits immediately for subsequent runs
- Run tracking:
  - Emit debug-only events for work logs (no downstream execution)
  - Tag with `debug` and `worklog`, include input/module/output/mode in payload
  - Link to underlying events by ID when needed

## Non-Functional Requirements
- None beyond local developer usability.
- Performance and security constraints are out of scope for now.

## Open Questions
- How should module-only execution be wired into core-rust (new endpoint or
  debug-only mode)?
- Should prompt edits be hot-reloaded or applied on the next run only?
