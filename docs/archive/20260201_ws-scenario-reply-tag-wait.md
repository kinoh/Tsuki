# Wait for reply-tagged events in ws_scenario

## Context
The Rust scenario client was advancing after any incoming event, including the
server echo of the user input. This caused the next input to be sent before a
user-facing reply was emitted.

## Decision
- Treat only events tagged with both `action` and `response` as replies.
- Advance the scenario only when such a reply event is observed.

## Rationale
- The event payload is intentionally schema-less, so tag-based detection is more
  stable than inspecting payload fields.
- The `emit_user_reply` tool is the only producer of `action` + `response` tags,
  making it a reliable signal for user-facing replies.
