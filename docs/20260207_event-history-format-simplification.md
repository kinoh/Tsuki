# Event History Format Simplification

## Context
- Debug/decision context history formatting was verbose and hard to scan.
- Requested format:
  - separate header
  - no turn field
  - role labels that encode submodule identity
  - local timestamp without milliseconds

## Decision
- Standardize history lines to:
  - `ts | role | message`
- Add a fixed header row:
  - `ts | role | message`
- Role mapping:
  - `user`
  - `submodule:<module_name>`
  - `decision`
  - `assistant`

## Why
- Keeps history compact while preserving actor identity.
- Makes chronological reading easier for both humans and model context debugging.

## Implementation Notes
- Convert event timestamps to local time and truncate to seconds.
- Keep message extraction from `payload.text` with fallback to truncated payload JSON.
