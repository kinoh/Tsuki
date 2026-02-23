# Fail Fast When prompts.path Is Invalid

## Context
Integration runs can specify `prompts_file` in runner config, which patches `config.toml` `[prompts].path`.
If prompt loading fails (parse/validation issues), runtime previously used `unwrap_or_default()` and silently fell back to config defaults.

This made prompt source ambiguous and hid misconfiguration, causing unexpected style/instruction behavior.

## Decision
In `core-rust/src/main.rs`, loading prompt overrides now fails fast:
- Replace silent fallback (`unwrap_or_default`) with a hard failure including path and error detail.

## Why
- Prompt source must be explicit and trustworthy in integration/debug runs.
- Silent fallback breaks reproducibility and makes failures hard to diagnose.
- Failing at startup surfaces invalid prompt files immediately.
