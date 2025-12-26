# Structured Memory build fix

## Decision
- Added `serde_json` as an explicit dependency because `rmcp` tool macros require the crate to be available in the dependency graph.
- Updated `rmcp` API usage to match 0.6.4 changes: switched to public `Parameters` wrapper import, added `meta` to `CallToolResult`, and used `Implementation::from_build_env()` to satisfy new fields.

## Notes
- No behavior changes to the service logic; changes are compatibility fixes for the current `rmcp` version.
