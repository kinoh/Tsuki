# Sensory camera client tool

- Added `tools/sensory_camera_client.py` to capture an image via `fswebcam`, request a concise description from OpenAI (vision through `chat/completions`), and forward it to Tsuki as a `sensory` WebSocket message. The client reproduces the `ws_client.js` protocol (initial `user:token` auth string, then JSON payload).
- Kept dependencies to the standard library plus the `fswebcam` binary by implementing a minimal RFC 6455 WebSocket client (handshake verification, masked client frames, ping/pong handling) and using `urllib` for the OpenAI call. Images are written to a temporary file and removed after use.
- Configurable via environment variables: `WS_URL`, `WEB_AUTH_TOKEN`, `USER_NAME`, `OPENAI_API_KEY`, `OPENAI_MODEL`, `OPENAI_BASE_URL`, `OPENAI_TIMEOUT_SECONDS`, `WS_REPLY_WINDOW_SECONDS`, and `FSWEBCAM_CMD`.
- No automated tests added or run; test changes require approval and none exist for this flow. Manual validation is recommended after providing valid credentials and camera access.
