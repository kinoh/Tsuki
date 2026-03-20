# Load backup via spawnSync

## Decision
- Use `spawnSync` for tar extraction to avoid shell invocation and to check the exit status explicitly.
- Throw a descriptive error when tar fails so failures are visible during development.

## Rationale
- `spawnSync` provides direct access to `status` and `error`, enabling explicit exit-code handling.
- Argument-based invocation avoids shell interpolation risks.

## Notes
- Change requested by the user to make exit-code handling explicit.
