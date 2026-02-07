# Debug LLM Context Observability

## Context
- Debug UI needs to verify whether history/submodule outputs are actually included in LLM input.
- Existing `llm.raw` event payload had model response details but not the full input context sent to the LLM.

## Decision
- Add `context` (full LLM input text) to `llm.raw` debug event payload.
- Add `output_text` alongside raw response for quick input/output inspection in one event.

## Why
- Keeps API surface unchanged.
- Uses existing debug event stream and existing raw response viewer.
- Enables direct validation of prompt composition and history inclusion.

## Implementation Notes
- `emit_debug_module_events` now receives the composed context string.
- `run_submodule_debug` and `run_decision_debug` pass their final `context` to the emitter.
