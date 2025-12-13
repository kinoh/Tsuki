# RSS MCP README updates

- Reframed the RSS MCP server as a lightweight feed reader: YAML feed list only, no OPML/DB, and a single `get_articles` tool.
- Chose TOON for responses (title, url, published_at, description) instead of Markdown to reduce parsing complexity.
- Standardized timestamps to the configured `TZ` environment variable.
- Included feed descriptions in responses when available; URLs remain the only required input in `rss.yaml`.
- Added optional `FEED_TIMEOUT_SECONDS` env var (default 2s) to bound per-feed HTTP fetch time.
- Implemented Rust MCP server (`mcp/rss`) with `get_articles` tool that honors `since`, `n`, `TZ`, and `FEED_TIMEOUT_SECONDS`, emitting TOON output.
