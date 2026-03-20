# Configurable Decision Input Context Template

## Decision
- Add `internal_prompts.decision_context_template` to `core-rust/config.toml`.
- Move the full decision input context structure into config and render it via placeholders at runtime.
- Keep only fact-style sections in the template:
  - `latest_user_input`
  - `active_concepts_from_concept_graph`
  - `outputs_from_immediately_executed_submodules`
  - `candidate_submodules_by_interest_match`
  - `recent_event_history`
- Do not include a separate hard-trigger module-name section, because output lines already carry submodule names.

## Rationale
- Keep behavioral guidance in module instructions, not in runtime input context.
- Remove hardcoded input-context structure from code so wording/layout is tunable without recompiling.
- Reduce duplicated information and keep decision input compact.

## User Feedback Incorporated
- Do not include directional guidance in input context.
- Keep semantic labels explicit and fact-only.
- Keep `latest_user_input` as the user-input section name.
- Use `recent_event_history` (not timeline wording).

## Follow-up Clarification
- The template belongs under `internal_prompts`, not a generic `input` section.
- Reason: decision/submodule context templates are LLM-facing wording assets, so they share responsibility with other internal prompt templates.
