# Core-Rust TTS Endpoint Restoration

## Overview
This document records the decision to restore `POST /tts` in `core-rust` after it had been explicitly excluded during the migration-first replacement phase.

Compatibility Impact: breaking-by-default is unchanged for removed thread/message APIs, but TTS API support is reintroduced.

## Problem Statement
The migration policy temporarily removed `POST /tts` to reduce cutover complexity.  
After cutover, operational and product needs required TTS again, and keeping it removed created an unnecessary feature gap versus the previous `core` runtime.

## Decision
- Reintroduce `POST /tts` in `core-rust`.
- Keep the same request/response contract shape used by `core`:
  - request body: `{ "message": string }`
  - success response: `audio/wav` binary
- Keep legacy-compatible error mapping:
  - invalid/empty message: `400`
  - VoiceVox stage failures: `502`
  - upstream timeout: `504`
  - unexpected failure: `500`
- Keep existing HTTP auth contract (`authorization: <user>:<WEB_AUTH_TOKEN>`).

## Why This Design
- Minimizes client-side change risk by restoring previously shipped API semantics.
- Keeps responsibility boundaries explicit:
  - HTTP validation/auth in server route
  - upstream synthesis orchestration in the same route module for now (consistent with current `server_app.rs` structure)
- Preserves fail-fast behavior and explicit error responses required by project policy.

## Implementation Notes
- `core-rust/src/server_app.rs`
  - Added `POST /tts` route.
  - Added request payload validation and auth checks.
  - Added upstream calls:
    1. `tts.ja_accent_url + "/accent"`
    2. `tts.voicevox_url + "/accent_phrases"`
    3. `tts.voicevox_url + "/synthesis"`
  - Added explicit `config.toml` fields:
    - `tts.ja_accent_url`
    - `tts.voicevox_url`
    - `tts.voicevox_speaker`
    - `tts.voicevox_timeout_ms`
- `api-specs/openapi.yaml`
  - Added `/tts` contract and `TtsRequest` schema.
  - Updated API version to `1.2.0`.

## Follow-up
- Add focused integration test coverage for `/tts` response codes and content type once upstream dependency stubs are available in CI.
