# VoiceVox TTS - Compose Revival Decisions

## Context
- Revive VoiceVox engine container in current TypeScript core stack.
- HTTP endpoint will return audio directly; implementation to follow later.

## Decisions
- Endpoint will be public but require authentication.
- Response format: `audio/wav` direct body (no JSON wrapper).
- Request will NOT accept `speaker`, `speed`, or `pitch` parameters.
- Compose will include `voicevox-engine` service (CPU image, 0.25.0) exposed on port 50021.

## Notes
- Future core HTTP route will call VoiceVox `audio_query` -> `synthesis` using internal defaults.
- User explicitly fixed the public/auth + response format + no parameterization scope.
