# Router history in prompt

## Rationale
- Router should see recent conversation context when evaluating sensory inputs.
- Keep Router independent: ActiveUser prepares history text and passes it as part of the routing input, rather than letting Router fetch state.

## Decisions
- Added `ConversationManager.getRecentMessages(limit)` to fetch recent messages of the current thread.
- ActiveUser now builds router history strings (role + truncated text) and passes them to `AIRouter.route` for sensory messages.
- Router prompt now fills `{{messages}}` with the provided history lines; default limit is 10, configurable via `ROUTER_HISTORY_LIMIT`.
- `MessageInput` supports optional `history` so routing can carry context without expanding Router’s dependencies.

## Notes
- History lines are truncated to keep prompt size modest; empty content is marked as `[empty]`.
- No tests added (behavioral change confined to routing prompt context).
