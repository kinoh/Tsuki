# RSS MCP README updates

- Reframed the RSS MCP server as a lightweight feed reader: YAML feed list only, no OPML/DB, and a single `get_articles` tool.
- Chose TOON for responses (title, url, published_at, description) instead of Markdown to reduce parsing complexity.
- Standardized timestamps to the configured `TZ` environment variable.
- Included feed descriptions in responses when available; URLs remain the only required input in `rss.yaml`.
- Added optional `FEED_TIMEOUT_SECONDS` env var (default 2s) to bound per-feed HTTP fetch time.
- Implemented Rust MCP server (`mcp/rss`) with `get_articles` tool that honors `since`, `n`, `TZ`, and `FEED_TIMEOUT_SECONDS`, emitting TOON output.
- Cleaned descriptions: strip HTML tags, normalize whitespace, and truncate to 280 characters with ellipsis to keep TOON rows compact.
- Added explicit binary name `rss` (matching directory) via Cargo `[[bin]]`.
- Swapped config location from `DATA_DIR` + `rss.yaml` to explicit `RSS_CONFIG_PATH` pointing at the YAML file.
- Core wiring: `core/src/mastra/mcp.ts` now launches `./bin/rss` with `RSS_CONFIG_PATH` and `TZ`, replacing `rss-mcp-lite`.
