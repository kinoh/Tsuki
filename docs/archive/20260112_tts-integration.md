# TTS Integration Split

## Context
- The current TTS endpoint is implemented inside `core/src/server/routes/tts.ts`.
- We want to prepare for future agent-side usage without introducing a generic services layer.

## Decisions
- Extract VoiceVox + ja-accent HTTP calls into `core/src/integrations/tts.ts`.
- Keep `core/src/server/routes/tts.ts` as the HTTP boundary that validates input and maps errors to HTTP responses.
- Avoid a `services` layer; use an `integrations` namespace to keep external system boundaries explicit.

## Notes
- This change is requested to align with future agent usage while preserving the current HTTP API behavior.
