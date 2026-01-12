# Admin Usage Tokens Display

## Overview
Added token usage visibility to AdminJS thread and message views.

## Problem Statement
The admin UI lacked visibility into token usage at both the thread and message level. Operators need quick totals in list views and full usage details in record views.

## Solution
- Expose aggregated token totals per thread in list views and full usage totals in thread show views.
- Expose per-message usage for assistant messages, showing totals in list views and full usage details in show views.

## Design Decisions
- Thread usage is loaded with a single query that joins usage_stats for aggregation and sorting.
- Message usage is matched to assistant messages by chronological order within a thread.
- Non-assistant messages show empty usage fields.
- Usage retrieval failures fall back to showing messages without usage rather than failing the view.

## Implementation Details
- ThreadResource aggregates usage from usage_stats grouped by thread_id.
- MessageResource fetches thread messages via API and applies usage entries from usage_stats ordered by created_at.
- AdminJS listProperties/showProperties updated to display total tokens in list views and all usage fields in show views.

## Future Considerations
- If usage_stats includes per-message identifiers, switch to direct joins instead of positional matching.
