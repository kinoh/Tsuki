# Integration Harness Partial Evaluation on Timeout

## Context
- Integration runs could fail with `EXEC_WS_ERROR` / `EXEC_TIMEOUT` after producing useful partial dialogue and event logs.
- Previous behavior assigned empty metrics, which effectively collapsed gate scores to `0.0` and discarded diagnostic value.

## Decision
- On execution-time failures, the harness now attempts a **partial judge evaluation** using the transcript and filtered events gathered up to failure.
- The run still remains failed (`pass=false`, `failure_code` unchanged), but metrics and judge summary are preserved when judge output is valid.
- For incomplete runs, `scenario_requirement_fit` is capped at `0.5` to keep strict completion semantics while retaining signal from partial quality.

## Why
- This keeps hard failure behavior intact for CI gating.
- It avoids wasting partial evidence and makes regression triage significantly more informative.
- The cap guarantees incomplete runs cannot accidentally pass full scenario requirements.
