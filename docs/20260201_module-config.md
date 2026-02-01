# Configure initial submodules via config.toml

## Decision
- Define initial submodules in `core-rust/config.toml` under `[[modules]]`.
- Use the config entries to seed the ModuleRegistry on boot.
- Each module entry includes `name`, `instructions`, and `enabled`.

## Rationale
- Submodule definitions are part of runtime configuration, not hardcoded defaults.
- This makes module changes explicit and traceable while keeping secrets out of config.
- The registry still owns persistence; config only seeds the initial state.

## Notes
- If a module already exists in the registry, `ensure_defaults` will not override it.
- `instructions` must remain concise and deterministic for predictable decision behavior.
