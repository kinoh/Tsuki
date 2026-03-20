# Debug Event Series Alignment with Normal Execution

## Context
- Current debug single-module runs do not produce the same primary event sequence as normal execution.
- Debug runs mainly emit `debug,worklog` and `debug,llm.raw`, while normal execution emits user/decision/reply series.
- As a result, debug-provided user inputs can be missing from subsequent context history.

## Goal
- Make debug execution follow the same primary event semantics as normal execution.
- Keep debug-only observability (`llm.raw`, context payloads) as supplemental events.
- Preserve replay/debug ergonomics (`cutoff`, `exclude`) without context loss.

## Current Behavior (Problem)
- Normal execution (WS path):
  - emits `user(input)` -> `submodule:*` -> `decision` -> `action,response` (if respond)
- Debug single-module execution:
  - emits `debug,worklog` and `debug,llm.raw`
  - may emit `action,response` only if tool is called
  - does not consistently emit `user(input)` and standard module events
- History builder ignores `debug` events, causing missing debug inputs in later context.

## Target Behavior
- Debug execution should emit primary events in normal format:
  - `user(input)` is created first (unless explicitly reusing an open input turn)
  - module output event is emitted in the normal role/tag shape
  - decision run emits normal `decision` event
  - reply remains normal `action,response`
- Work log should include a visible row for debug-appended `user(input)` so cutoff/exclude can be controlled from UI.
- Work log exclude/cutoff controls should map to primary history event ids/timestamps (not only debug worklog event ids).
- Debug observability remains additive:
  - `debug,llm.raw` includes raw response and composed LLM context
  - these debug events stay excluded from context history

## Optional Input-Append Policy
- Add debug request policy to control user-input event insertion:
  - `always_new` (default): always append a new `user(input)` event
  - `reuse_open`: if an input event after cutoff is not yet closed by a `decision` event, reuse it
- Motivation:
  - supports both "new turn per run" and "incremental run in same turn" workflows

## Cutoff/Exclude Interaction
- Existing controls are retained:
  - `history_cutoff_ts`: defines lower bound for included history (`event.ts >= cutoff`)
  - `exclude_event_ids`: explicitly omitted events
- Alignment change does not alter these controls; it changes what primary events are emitted during debug runs.
- Debug UI controls should carry `history_event_id` and `history_event_ts` from worklog payload when available.

## Non-Goals
- No destructive event deletion.
- No major API redesign for debug sessions/forks at this stage.

## Acceptance Criteria
- A debug run with input `X` emits a normal `user(input)` event path (per policy).
- Subsequent debug decision runs can see prior debug-provided user input in `Recent event history`.
- Event history format remains:
  - `ts | role | message`
  - roles include `user`, `submodule:<name>`, `decision`, `assistant`
- `debug,llm.raw` remains available and excluded from history input selection.
- Decision debug context should not duplicate "latest input" outside history.
- User-provided submodule outputs for decision debug should be represented as events and therefore appear via `Recent event history`.

## Why This Direction
- Restores semantic consistency between normal and debug execution.
- Eliminates context gaps while keeping debug observability rich.
- Minimizes API expansion and avoids irreversible operations.
