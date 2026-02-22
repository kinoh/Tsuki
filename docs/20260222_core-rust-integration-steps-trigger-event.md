# Core-Rust Integration Harness: Step Sequence with Fixed Trigger Event

Date: 2026-02-22

## Context
- We needed integration scenarios that can emit self-improvement trigger events in a deterministic place.
- The existing integration harness only supported one continuous tester conversation driven by top-level `tester_instructions`.
- A naive `steps` design risked degenerating into single-turn fragments and breaking the existing multi-turn conversation behavior.

## Decision
- Introduce `steps` in integration scenarios as a **phase sequence**, not a single-turn sequence.
- Add two step kinds:
  - `conversation`: multi-turn tester-driven dialogue with its own `tester_instructions` and optional `max_turns`.
  - `emit_event`: fixed runtime event emission (currently only `event.type: trigger`), followed by explicit wait for completion tags.
- Keep backward compatibility:
  - If `steps` is omitted, the harness uses legacy top-level `tester_instructions` as one conversation phase.
- Keep conversation completion token fixed to `__TEST_DONE__` (not configurable).

## Why
- Deterministic event injection is necessary to reliably verify self-improvement flow in integration tests.
- Conversation must remain multi-turn to preserve realism and existing tester/judge behavior.
- Phase-based steps allow controlled trigger insertion without rewriting tester logic into brittle command parsing.

## Implementation Notes
- `core-rust/examples/integration_harness.rs`
  - Scenario schema extended with optional `steps`.
  - Added runtime step planning with validation and legacy fallback.
  - Added `emit_event` execution over the same WebSocket session.
  - Added wait loop for emitted-event completion via DB polling.
- `core-rust/tests/integration/README.md`
  - Added step schema documentation and defaults.
- `core-rust/tests/integration/scenarios/self_improvement_trigger.yaml`
  - Added scenario example using conversation -> emit_event(trigger) -> conversation.

## Compatibility Impact
- breaking-by-default (no compatibility layer): No.
- Effective behavior remains backward compatible for existing scenarios without `steps`.
