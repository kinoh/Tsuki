# WebSocket Tool Logging Alignment

## Overview
Align WebSocket response `chat` formatting with HTTP `/threads/:id` so tool invocations appear inline in assistant messages. This provides consistent GUI rendering for tool-using scenarios.

## Problem Statement
Tool usage appears in HTTP thread history but not in WebSocket client logs. This makes tool-driven scenarios hard to inspect in the WebSocket test client and causes mismatch with the GUI display that already relies on `[tool-invocation]` lines.

## Solution
Build the WebSocket `chat` array from `uiMessages` and reuse the same text extraction logic that formats tool entries. When `TRACE_TOOLS` is enabled, append tool arguments/results on the same line. If `uiMessages` are not available, fall back to the existing `response.text` handling.

## Design Decisions
- **No `role: tool` messages**: Tool events are represented as strings inside the assistant `chat` array, matching `/threads/:id` behavior and existing GUI rendering.
- **Optional deep tracing**: `TRACE_TOOLS` appends tool args/results inline and flags errors, enabling richer debugging without always exposing tool payloads.

## Implementation Details
- Build `chat` from `response.response.uiMessages[].parts` when available.
- Use the shared text extraction to format tool entries:
  - `tool-invocation` renders as `[tool-invocation] <toolName>`.
  - `tool-*` renders as `[tool-<name>]` (type string as-is).
  - `(error)` is appended when a tool result payload indicates failure.
  - With `TRACE_TOOLS=1`, append tool `args` (call) or `result` (output) JSON on the same line.
- If `uiMessages` is absent, use `response.text` with the existing JSON-splitting behavior.
- `TRACE_TOOLS` enables args/result output.
- Read `TRACE_TOOLS` (and other environment flags if needed later) via `ConfigService`, not directly in the responder.

## Future Considerations
- Consider configurable formatting prefixes for args/results (e.g., `args:` / `result:`) if readability becomes an issue.
- If `uiMessages` shape changes in upstream Mastra/AI SDK, add a compatibility layer or unit checks. 
