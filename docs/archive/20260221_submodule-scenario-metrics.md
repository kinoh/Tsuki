# Submodule scenario metrics for short-turn integration tests

## Context
A new integration scenario was added to evaluate submodule behavior in short conversations.
The user explicitly narrowed the scope to practical metrics that can be judged in few turns.

## Decision
Use four submodule-focused metrics in `core-rust/tests/integration/scenarios/router_submodule_behavior.yaml`:
- `submodule_trigger_precision`
- `submodule_trigger_recall`
- `submodule_noninterference`
- `conversation_flow_preservation`

## Why
- `assertiveness` calibration was rejected for this scenario because it is hard to validate reliably in short runs.
- Leakage checks were deprioritized because they are not always required by product intent.
- Multi-submodule conflict-resolution was skipped because it is expensive and unstable to force in short-turn natural chat.

The selected four metrics can be judged from currently available evidence in event logs:
router trigger fields, decision outputs, and final user-facing replies.

## Notes
This scenario still includes baseline gates (`scenario_requirement_fit`, `dialog_naturalness`) because the integration harness requires them.
