# RSS MCP server for LLM agents

## Overview

- Lightweight RSS fetcher that merges multiple feeds into a single response
- Configuration uses a simple YAML feed list (no OPML, no DB)
- Single tool (`get_articles`) returns recent articles in TOON format with description snippets
- All timestamps are normalized to the configured `TZ`

## Configuration

### Environment Variables

- **DATA_DIR** (Required): Directory that contains `rss.yaml` and runtime data
  - Example: `export DATA_DIR="/path/to/data"`
  - Should be an absolute path for consistent resolution
- **TZ** (Required): Timezone used to format article timestamps
  - Example: `export TZ="Asia/Tokyo"`
  - Must be a valid IANA timezone
- **FEED_TIMEOUT_SECONDS** (Optional): Per-feed HTTP timeout in seconds
  - Example: `export FEED_TIMEOUT_SECONDS=2`
  - Default: `2`

### Feed List

Feeds are defined in `${DATA_DIR}/rss.yaml`:

```yaml
feeds:
  - https://example.com/feed.xml
  - https://techcrunch.com/feed/
  - https://www.theverge.com/rss/index.xml
```

Only URLs are required; site names and metadata are derived from the feed content.

### Network Requirements

- Outbound HTTP/HTTPS access to each configured RSS feed is required.

## Tools

### get_articles

#### Arguments

- `since` (optional): RFC3339 timestamp (`2025-12-13T11:22:33Z`)
- `n` (optional): Number of articles to get. Default: 20

#### Response

Returns TOON-formatted articles:

```
articles[2]{title,url,published_at,description}:
  Example Title,https://example.com/post,2025-12-13T20:00:00+09:00,Short summary...
  Another,https://another.com/post,2025-12-12T08:15:00+09:00,Short summary...
```

- `title`: Article title (empty string when missing)
- `url`: Canonical or link URL from the feed entry
- `published_at`: Timestamp in the configured `TZ` (falls back to feed-provided date or omitted when unavailable)
- `description`: Short summary derived from feed `description`/`content` when available (may be empty). HTML tags are stripped and content is truncated to 280 characters with an ellipsis when longer.

#### Errors

- `Error: since: invalid timestamp`
- `Error: fetch: upstream request failed`
- `Error: config: feeds not configured`
