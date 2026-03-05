# Events Tags Filter for GUI Message Fetch

## Overview
Added `tags` query support to `GET /events` in `core-rust` so clients can request display-target events (`response`, `input`) when needed.

## Problem Statement
GUI previously fetched the latest N raw events and then discarded non-display events locally.
When debug/observe/state events were dense, visible message count became lower than requested (`limit=20` often rendered far fewer than 20).

## Solution
- Extended `EventsQuery` with `tags: Vec<String>`.
- Implemented OR matching: an event is included when it has at least one requested tag.
- Kept backward compatibility for callers that do not pass `tags` (same behavior as before).
- Added server-side batched scan when `tags` is provided to fill `limit` after filtering.
- Kept GUI unchanged in this change set; adoption is deferred to explicit GUI work.

## Design Decisions
- Single parameter only (`tags`), no include/exclude split.
  - Reason: user requirement explicitly rejected dual-parameter design.
- No `text_only` filter.
  - Reason: rejected because it breaks multimodal extensibility.
- OR semantics for `tags`.
  - Reason: GUI chat fetch needs either `response` or `input`, not intersection.

## Implementation Details
- Server file: `core-rust/src/server_app.rs`
  - `EventsQuery.tags` added.
  - Supports repeated `tags` query keys (`style: form`, `explode: true`), such as `tags=input&tags=response`.
  - `normalize_event_tags` added for trim/lowercase/dedup.
  - `event_has_any_tag` added (case-insensitive match).
  - `list_events_with_tags` added to fetch in batches and filter server-side until `limit` or scan cap.

## Numeric Targets and Limits
- Target: return up to requested `limit` after filtering, not before filtering.
- Batch size policy: `clamp(limit * 4, 50, 500)`.
- Scan cap: `5000` events per request to avoid unbounded DB scans.

## Compatibility Impact
Breaking-by-default policy is unchanged for `core-rust`, but this specific API change is additive:
existing clients without `tags` keep previous behavior.

## User-Driven Clarifications Captured
- `/admin` debug filtering is out of scope; only `/gui` and `/events` matter.
- Internal mode should omit `tags` instead of forcing include lists.
- New tags should be handled by updating GUI include list explicitly.
- Compatibility fallback for legacy tag names is not required.
- Do not change GUI without explicit request; API-side work and GUI-side adoption are separated.
