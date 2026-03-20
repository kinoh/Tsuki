# Gate temperature for Response API models

## Context
The Rust core uses the Responses API for all modules. Some GPT-5 models (for example
`gpt-5-mini`) reject the `temperature` parameter, which caused runtime failures in
E2E scenario runs.

## Decision
- Add `llm.temperature_enabled` to the Rust core config.
- Only send `temperature` when this flag is `true`.
- Keep `temperature` itself as a required numeric value, so the config remains
  explicit even when the flag is off.

## Rationale
- Avoids implicit retries or hidden fallbacks.
- Keeps configuration strict while allowing model-specific compatibility.
- Makes the behavior explicit and easy to audit in `config.toml`.
