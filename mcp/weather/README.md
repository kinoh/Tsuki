# Weather MCP server for LLM agents

## Overview

- Retrieves short-term weather forecasts from a fixed `https://tenki.jp/forecast/{location}` page and returns them in Markdown for LLM consumption
- Provides today's and tomorrow's outlook including temperature and wind details extracted from the tenki.jp forecast page
- Designed as a drop-in MCP server for Tsuki; follows the conventions used by other servers under `mcp/`

## Configuration

### Environment Variables

- **LOCATION_PATH** (Required): tenki.jp path served by the MCP tool
  - Example: `export LOCATION_PATH="3/16/4410/13104/"`
  - Obtain the value by opening a forecast page on tenki.jp and copying the path segment that follows `/forecast/`
  - Mapping path segments to human-readable regions is outside the scope of this server; consumers should maintain that mapping separately

### Network Requirements

- Outbound HTTPS access to `tenki.jp` is required
- The server checks `https://tenki.jp/robots.txt` before requesting a forecast page and aborts if access is disallowed

## Features

- robots.txt-aware fetching to respect site access policies
- HTML-to-Markdown conversion tailored for LLM readability
- Focused extraction that summarizes today's and tomorrow's forecast sections
- Fixed-location design keeps the MCP contract simple and avoids region lookup logic

## Usage Patterns

### Basic forecast retrieval

```json
{
  "tool": "get_weather",
  "arguments": {}
}
```

Arguments are ignored; the configured location is always returned.

## Tools

### get_weather

Retrieves the Markdown-formatted forecast for the requested location.

#### Arguments

- None.

#### Response

Markdown document with a headline describing the location and measurement timestamp, followed by tables for today's and tomorrow's forecast (temperature highs/lows, precipitation, wind).

#### Errors

- Returns `Error: disallowed by robots.txt` when tenki.jp forbids crawling the target page
- Returns `Error: request failed` when the HTTP request cannot be completed
- Returns `Error: failed to parse` when forecast sections cannot be extracted from the page

## Implementation Notes

- The Rust MCP server should follow the structure used in `mcp/scheduler` (Tokio runtime, tool registry, MCP protocol endpoints)
- Prefer `reqwest` for HTTP fetching and `scraper`/`select` for HTML parsing, keeping the Markdown output identical to the current proof of concept
- The Rust implementation should not expose dynamic location switching; it should read the configured `LOCATION_PATH` during startup and serve that location exclusively
