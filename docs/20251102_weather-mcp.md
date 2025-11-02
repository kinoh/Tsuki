# Weather MCP Implementation Notes

- Fix the location to a single `LOCATION_PATH`; mapping path segments to human-readable regions should stay outside the MCP server.
- Respect `robots.txt` by building the robots URL manually (`set_path("/robots.txt")`) instead of naïvely appending `/robots.txt` to the forecast URL.
- Always set an explicit `User-Agent` on outgoing requests; tenki.jp rejects empty agents with HTTP 403.
- Apply short connect and read timeouts to the HTTP client so negative connectivity tests terminate promptly.
- Avoid fragile cross-section regexes for forecast extraction; derive the today/tomorrow sections from their headings and trim each block at the first `最大風速` line to match real-world Markdown.
