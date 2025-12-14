# Router test CLI

## Rationale
- Allow quick, isolated evaluation of AIRouter without spinning up the full core stack or AgentService.
- Use MCP RSS once per run to prepare sensory samples; keep cost and runtime minimal (single call, capped items).
- Avoid SensoryService coupling; test flow is CLI-only and deterministic from saved JSONL.

## Decisions
- Added `core/scripts/test_router.ts` with two modes:
  - `prepare`: call universal MCP `rss/get_articles` (default limit 20), convert TOON lines to sensory texts tagged with `source`/`time`, save as JSONL.
  - default: read JSONL and stream each sensory to AIRouter, printing route decisions with snippets.
- Options: `--out`, `--limit`, `--feed-id` for prepare; `--in`, `--limit`, `--model`, `--max-log`, `--instructions-file` for run. Falls back to encrypted prompt; if missing, uses minimal router instructions.
- Parsing is lenient: TOON lines are split by newline, header lines skipped, timestamp extracted via ISO-ish regex; no strict dedupe beyond per-run duplicate lines.

## Notes
- Requires MCP binaries present for prepare (`bin/rss` via `getUniversalMCP`); network access for RSS feeds.
- Requires `OPENAI_API_KEY` when running router decisions; `ROUTER_MODEL` optional (defaults to `gpt-4o-mini`).
