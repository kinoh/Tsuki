# Router-First Concept Activation in core-rust

## Context
The runtime in `core-rust` previously executed all active submodules on every input before running decision. This caused unnecessary fan-out and did not match the intended router-first orchestration.

## Decision
- Introduced a router-first runtime path in `core-rust`:
  - On each input, router computes `concept_activation` and `soft_recommendations` first.
  - Decision input composition now includes router output plus event history.
- Stopped always-on submodule fan-out in the normal input flow.
- Exposed submodules to Decision as callable tools (`run_submodule__{name}`), so submodules execute only when Decision explicitly calls them.
- Added router threshold and top-N policy to config files under `[router]`:
  - `concept_top_n`
  - `recommendation_threshold`
- Updated `submodule_outputs` override behavior in decision debug context composition:
  - Override existing matching submodule events.
  - If a matching event is missing, insert a synthetic context-only submodule output at the latest user-input position.

## Why
- Router-first orchestration reduces unnecessary cost/latency from unconditional submodule execution.
- Keeping submodule execution as Decision tool calls preserves soft recommendation semantics (recommendation is guidance, not forced execution).
- Config-managed thresholds allow tuning without code edits.
- Context-only insertion for missing `submodule_outputs` matches the debug composition contract while avoiding synthetic persistence events.

## Notes
- Concept activation access remains in-process via application state store (no MCP round trip in the activation path).
- Added a unit test for the override-and-insert rule to protect the new context composition behavior.
- Follow-up update after user correction:
  - The initial implementation omitted router `hard_triggers`.
  - We aligned implementation with the updated spec by adding hard-trigger thresholds in config, router hard-trigger output, application-stage hard-trigger execution before decision, and hard-trigger execution results in decision input.
