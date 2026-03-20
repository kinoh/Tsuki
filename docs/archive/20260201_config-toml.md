# Config file requirement for non-secret settings

## Decision
- Move non-secret runtime settings to `core-rust/config.toml` and require all of them to be present.
- Read secrets only from environment variables (`WEB_AUTH_TOKEN`, `OPENAI_API_KEY`, and `TURSO_AUTH_TOKEN` when `db.remote_url` is set).
- Keep module prompt definitions in code for now; base personality and decision instructions are configured via TOML.

## Rationale
- Avoid implicit defaults and make runtime behavior explicit and reproducible.
- Separate secrets from non-secret configuration to prevent accidental leakage in source control.
- TOML keeps configuration readable while allowing structured grouping (server/llm/limits/db).

## Notes
- `config.toml` is required at runtime; missing or malformed files should fail fast.
- `db.path` is always required so local storage exists even when using a remote replica.
