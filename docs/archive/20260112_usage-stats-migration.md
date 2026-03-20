# Usage Stats LanguageModelUsage Migration

## Overview
Aligned usage stats storage with Mastra's LanguageModelUsage and removed the redundant timestamp column.

## Problem Statement
The usage table stored prompt/completion token counts and a separate timestamp column. Mastra's LanguageModelUsage now provides input/output tokens plus additional fields (reasoning, cached input), and the response usage is not optional. This mismatch risked silent type drift and incomplete persistence.

## Solution
- Update the usage_stats schema to persist the numeric LanguageModelUsage fields.
- Remove the redundant timestamp column and index by created_at.
- Add a column-count-based migration to map legacy prompt/completion tokens to input/output tokens.
- Require LanguageModelUsage in recordUsage to surface type changes at compile time.

## Design Decisions
- Migration trigger relies only on column count as requested; name mismatches are not detected.
- Raw usage data is no longer persisted; only LanguageModelUsage numeric fields are stored.
- created_at is the single timestamp source; legacy timestamp is dropped.

## Implementation Details
- UsageStorage checks PRAGMA table_info for the expected column count (10).
- Migration renames the old table, creates the new schema, copies data, drops the old table, and recreates indexes.
- Migration maps legacy prompt/completion tokens to input/output tokens without a raw column.
- recordUsage now accepts LanguageModelUsage as a required field and inserts nullable token values explicitly.

## Future Considerations
- If LanguageModelUsage adds fields, update the schema and migration mapping accordingly.
