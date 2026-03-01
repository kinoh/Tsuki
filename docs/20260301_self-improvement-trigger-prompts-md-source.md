# Self-Improvement Trigger Instructions from prompts.md

## Context
- `core-rust` still had a hardcoded fallback (`DEFAULT_SELF_IMPROVEMENT_TRIGGER_INSTRUCTIONS`) for self-improvement trigger worker instructions.
- This contradicted the prompt-source policy used in the rest of runtime prompt loading, where prompt text should come from `prompts.md` and fail fast when invalid/missing.
- The hardcoded path also made prompt ownership split across files (`main.rs` + `prompts.md`), which increased drift risk.

## Decision
- Extend `prompts.md` schema with a required top-level section:
  - `# Self Improvement Trigger`
- Parse and persist it through `PromptOverrides.self_improvement_trigger`.
- Remove hardcoded default trigger instructions from `main.rs`.
- Require non-empty `Self Improvement Trigger` in prompt validation at load/write time.
- At trigger execution time, read instructions from loaded prompt overrides.

## Why
- Keeps prompt source explicit and unified in one prompt file.
- Removes hidden fallback behavior and enforces fail-fast semantics.
- Makes self-improvement worker prompt updates follow the same operational path as other prompt text.

## Implementation Notes
- `core-rust/src/prompts.rs`
  - Added `self_improvement_trigger` field.
  - Added `# Self Improvement Trigger` read/write handling.
  - Added required-section validation for non-empty trigger instructions.
- `core-rust/src/main.rs`
  - Removed hardcoded default string.
  - Startup now validates `Self Improvement Trigger` presence through `prompts.md`.
  - Prompt admin payload now carries `self_improvement_trigger`.
- `core-rust/src/application/improve_service.rs`
  - Trigger worker now reads instructions from prompt overrides.
  - If missing/empty at runtime, processing fails with `TRIGGER_INSTRUCTIONS_MISSING`.

## Compatibility Impact
breaking-by-default (no compatibility layer)

