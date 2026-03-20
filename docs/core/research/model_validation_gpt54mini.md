# Model Validation: GPT-5.4 mini

Date: 2026-03-20

## Objective

Evaluate whether `gpt-5.4-mini` can replace `gpt-5.2` in the following roles:
- **Phase 1**: Core LLM only (tester/judge remain at gpt-5.2)
- **Phase 2**: Core + Tester (judge remains at gpt-5.2)

Primary motivation: gpt-5.4-mini is cheaper than gpt-5.2, and tester has no tool use, making substitution plausible.

## Setup

| Component | Phase 1 | Phase 2 |
|---|---|---|
| Core | `gpt-5.4-mini` | `gpt-5.4-mini` |
| Tester | `gpt-5.2` | `gpt-5.4-mini` |
| Judge | `gpt-5.2` | `gpt-5.2` |

Config change: `config.toml [llm].model` edited directly. No harness code changes required.

Scenarios: Chitchat, Schedule_Setup, Fuzzy_Concept_Intro_Query, Self_Improvement_Trigger.
Run count: 3 per scenario.

## Phase 1 Results: Core = gpt-5.4-mini

All 4 scenarios passed.

| Scenario | overall_pass | SRF mean | DN mean | Notes |
|---|---|---|---|---|
| Chitchat | ✅ True | 0.92 | 0.82 | 3/3 runs pass |
| Schedule_Setup | ✅ True | 1.00 | 0.82 | 3/3 runs pass |
| Fuzzy_Concept_Intro_Query | ✅ True | 0.80 | 0.82 | 2/3 runs initially; fixed after metric correction (see below) |
| Self_Improvement_Trigger | ✅ True | 0.80 | 0.85 | 2/3 run-level pass; overall_pass=true |

### Metric fix: `disambiguation_naturalness` in Fuzzy_Concept_Intro_Query

Initial run produced overall_pass=False due to `disambiguation_naturalness=0.0` in run 2.

Root cause: the metric description was phrased around scoring a "closing question or clarification",
so the judge scored 0.0 when no question existed ("no evidence to score").
The intent was to penalize rigid "A or B?" binary narrowing — absence of such a question should score 1.0.

Fix: rewrote the metric description to make the default case (no question) explicit as 1.0.
Commit: `fb07348` — `fix(integration): clarify disambiguation_naturalness metric definition`

After fix: 3/3 pass, `disambiguation_naturalness` mean=1.00.

### Baseline comparison (gpt-5.2, recent runs)

| Scenario | gpt-5.2 pass rate | gpt-5.4-mini pass rate |
|---|---|---|
| Chitchat | 5/6 (83%) | 3/3 (100%) |
| Schedule_Setup | 2/2 (100%) | 3/3 (100%) |
| Fuzzy_Concept_Intro_Query | 1/1 (100%) | 3/3 (100%) |
| Self_Improvement_Trigger | 1/1 (100%) | overall_pass=true |

**Conclusion: Core replacement with gpt-5.4-mini is viable.**

## Phase 2 Results: Tester = gpt-5.4-mini

3 of 4 scenarios failed.

| Scenario | overall_pass | Failing gate | Root cause |
|---|---|---|---|
| Chitchat | ❌ False | `identity` mean=0.60 | Tester combined multiple missions in one turn |
| Schedule_Setup | ❌ False | `schedule_contract_alignment` mean=0.33 | Tester omitted ID `schedule_setup_daily` from creation request; core used its own ID; deletion request failed |
| Fuzzy_Concept_Intro_Query | ❌ False | `scenario_requirement_fit` mean=0.65 | Tester skipped turn 1 (light opener) and started directly from concept query |
| Self_Improvement_Trigger | ✅ True | — | Passed |

### Failure analysis

All failures are tester-side instruction following issues, not core behavior issues:

- **ID propagation**: tester instructions specified `id: schedule_setup_daily`, but gpt-5.4-mini
  rephrased the request without the ID. Core created `morning_message_4am`, then deletion of
  `schedule_setup_daily` failed.
- **Turn plan order**: tester skipped required opener turns, violating the strict turn sequence.
- **Mission batching**: tester combined multiple missions into one utterance despite the prompt
  stating "A utterance must aim to achieve only one mission at most."

### API parameter comparison (call_llm vs core)

Both use `CreateResponseArgs` via the OpenAI Responses API. Differences:

| Parameter | tester `call_llm` | core `ResponseApiAdapter` |
|---|---|---|
| `temperature` | not set | not set (temperature_enabled=false) |
| `max_output_tokens` | not set | 10000 |
| `tools` | none | emit_user_reply, schedule_*, etc. |
| `previous_response_id` | not set | set (multi-turn context) |

The API parameter differences do not explain the instruction following degradation.
The failures reflect a genuine capability difference in structured instruction adherence
between gpt-5.2 and gpt-5.4-mini for this role.

**Conclusion: Tester replacement with gpt-5.4-mini is not viable at this time.**

## Final State

- `config.toml [llm].model` = `gpt-5.4-mini` (Phase 1 validated, keep)
- `tests/integration/config/runner.toml [tester].model` = reverted to `gpt-5.2`
- `tests/integration/config/runner.toml [judge].model` = unchanged `gpt-5.2`
