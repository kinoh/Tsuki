# Integration Test Secrets

Place encrypted secret snippets as `.age` files in this directory.

Example:
- `persona_main.age`

Scenario placeholders resolve as:
- `{{persona_main}}` -> `tests/integration/secrets/persona_main.age`

Do not commit private key material. Keep identity key files outside the repository,
or ignore local key files via `.gitignore`.
