# Android env management with dotenvx

## Decision
- Use dotenvx in `gui` scripts to load Android build variables from `.env.local`.
- Remove hardcoded Android/FCM values from `.vscode/tasks.json`.
- Provide a tracked `.env.example` listing required variables.

## Rationale
- Prevents user-specific paths and API keys from being committed.
- Keeps the required variable list visible in the repo without leaking secrets.
- Centralizes environment loading in `gui/package.json` scripts.

## Notes
- User corrected the package name to `@dotenvx/dotenvx` after initial lookup.
