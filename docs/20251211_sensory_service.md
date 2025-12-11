# Sensory service inside core

## Rationale
- Run sensory ingestion inside `core` (no external process) since web-available data is sufficient.
- Use existing `PERMANENT_USERS` as sensory targets to keep configuration minimal.
- Keep the router concept of passing repeated sensory inputs; avoid strict dedupe.
- Interpret `SENSORY_POLL_MS` in seconds for simplicity (despite the name), defaulting to 60s.

## Decisions
- Added `SensoryService` (`core/src/agent/sensoryService.ts`) with a simple interval poller that forwards sensory text to `AgentService.processMessage` as `type: 'sensory'`.
- Placeholder fetcher emits faux headlines; tags appended are limited to `source` and `time` in the text payload.
- Wired `SensoryService` in `core/src/index.ts` after AgentService startup; uses `PERMANENT_USERS` and `SENSORY_POLL_MS`.
- Documented `SENSORY_POLL_MS` (seconds) in `AGENTS.md` config section.

## Notes
- No deduplication beyond router behavior; variability is intentional.
- No tests added (documentation and service wiring only).
