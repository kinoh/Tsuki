# Core-Rust Event Definition Cards (Current Runtime Contract)

## Overview
This document defines event cards for currently emitted event kinds in `core-rust`.
The goal is to make ownership, producer/consumer boundaries, and history inclusion policy explicit.

Compatibility Impact: `breaking-by-default (no compatibility layer)`.

## Scope
- Runtime and debug event emission implemented under `core-rust/src/`.
- Event kind here means the semantic identity used by consumers, currently represented by `meta.tags` (and partially by `source`).
- This document describes current behavior first. It does not introduce schema changes by itself.

## Global Rules (Current)
- Event envelope is `event_id`, `ts`, `source`, `modality`, `payload`, `meta.tags`.
- `meta.tags` currently mixes:
  - primary semantic kind (`decision`, `self_improvement.proposed`, ...)
  - behavior flags (`debug`, `error`, `observe`)
  - dimensions (`mode:*`, `tool:*`, `type:*`, `module:*`, `outcome:*`)
- LLM history assembly excludes events tagged with `debug` or `observe`.

## Card: `input`
- Kind: `input` (with `type:*` secondary tag)
- Layer: domain
- Owner: `application/debug_service`
- Producer:
  - `parse_and_append_input` (websocket ingress)
  - `maybe_append_debug_input_event` (debug run path)
- Consumers:
  - `history_service` (`is_user_input_event`, history formatting)
  - debug flow reuse-open logic
- Trigger Condition: accepted input message (`message`, `sensory`, `scheduler_notice`)
- Source/Modality: `source=user|system`, `modality=text`
- Tags:
  - Primary: `input`
  - Secondary: `type:message|type:sensory|type:scheduler_notice`
- Payload Contract:
  - Required: `text:string`
- History Policy: include = yes (except `source=system` may be interpreted differently by role mapping)
- Reliability/Semantics:
  - Idempotency: none
  - Ordering: best effort stream order only
  - Correlation: none

## Card: `response`
- Kind: `response`
- Layer: domain
- Owner: `tools` (`emit_user_reply` tool)
- Producer: `StateToolHandler::handle_inner` for `emit_user_reply`
- Consumers:
  - event log / websocket clients
  - role rendering in `history_service` (`assistant`)
- Trigger Condition: decision/runtime calls `emit_user_reply` tool
- Source/Modality: `source=assistant`, `modality=text`
- Tags:
  - Primary: `response`
- Payload Contract:
  - Required: `text:string`
- History Policy: include = yes
- Reliability/Semantics:
  - Idempotency: none
  - Correlation: indirect by surrounding decision/submodule flow

## Card: `decision`
- Kind: `decision`
- Layer: domain
- Owner: `application/execution_service`
- Producer:
  - `run_decision`
  - `run_decision_debug`
- Consumers:
  - `history_service` role mapping (`decision`)
  - debug run reuse-open logic (`is_decision_event`)
- Trigger Condition: decision module response parsed
- Source/Modality: `source=decision`, `modality=text`
- Tags:
  - Primary: `decision`
  - Secondary: optional `error`
- Payload Contract:
  - Required: `text:string`
  - Current text format: `decision=<...> reason=<...>`
- History Policy: include = yes
- Reliability/Semantics:
  - Idempotency: none
  - Correlation: implicit via temporal adjacency

## Card: `submodule`
- Kind: `submodule`
- Layer: domain
- Owner: `application/execution_service`
- Producer:
  - `run_submodule_debug`
  - `run_module` when `role_tag=submodule`
- Consumers:
  - `history_service` role mapping and submodule override logic
- Trigger Condition: submodule execution success/failure
- Source/Modality: `source=submodule:<name>`, `modality=text`
- Tags:
  - Primary: `submodule`
  - Secondary: optional `error`, optional `module:*`
- Payload Contract:
  - Required: `text:string`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation: module identity in `source` (`submodule:<name>`)

## Card: `router`
- Kind: `router`
- Layer: control
- Owner: `application/router_service`
- Producer: `run_router`
- Consumers:
  - debug/event log readers
  - runtime observability and diagnosis
- Trigger Condition: router stage completes
- Source/Modality: `source=router`, `modality=state`
- Tags:
  - Primary: `router`
- Payload Contract:
  - Required: router output object (`activation_query_terms`, `hard_triggers`, `soft_recommendations`, ...)
- History Policy: include = yes (not filtered by debug/observe)
- Reliability/Semantics:
  - Idempotency: none
  - Correlation: input-level temporal grouping only

## Card: `self_improvement.run`
- Kind: `self_improvement.run`
- Layer: control
- Owner: `application/improve_service`
- Producer:
  - `scheduler_service` self-improvement action event
  - `trigger_ingress_api`
  - `debug_service` trigger ingress (`type=trigger`)
- Consumers:
  - `improve_service::start_trigger_consumer`
- Trigger Condition: explicit trigger ingress or scheduler fire
- Source/Modality: `source=scheduler|system`, `modality=text`
- Tags:
  - Primary: `self_improvement.run` (or arbitrary trigger event name from ingress)
- Payload Contract:
  - Recommended: `target:string`, `reason:string`
  - Scheduler path also sets: `schedule_id`, `scheduled_at`, `created_at`, `created_by`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation: `event_id` becomes `trigger_event_id` in downstream events
  - Note: current trigger ingress accepts arbitrary event names; runtime consumer handles only `self_improvement.run`

## Card: `self_improvement.module_processed`
- Kind: `self_improvement.module_processed`
- Layer: control
- Owner: `application/improve_service`
- Producer: `emit_module_processed_event`
- Consumers:
  - integration harness/scenarios
  - operations/debug analysis
- Trigger Condition: each target module processed by self-improvement worker
- Source/Modality: `source=self_improvement`, `modality=text`
- Tags:
  - Primary: `self_improvement.module_processed`
- Payload Contract:
  - Required: `trigger_event_id`, `module_target`, `status`, `memory_updated`, `concept_graph_updated`, `processed_at`
  - Optional: `concept_ensured`, `proposal_id`, `error_code`, `error_detail`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation: `trigger_event_id`

## Card: `self_improvement.trigger_processed`
- Kind: `self_improvement.trigger_processed`
- Layer: observability
- Owner: `application/improve_service`
- Producer: `emit_trigger_processed_event`
- Consumers:
  - integration harness wait conditions
  - operations/debug analysis
- Trigger Condition: self-improvement trigger orchestration finalization
- Source/Modality: `source=self_improvement`, `modality=text`
- Tags:
  - Primary: `self_improvement.trigger_processed`
  - Secondary: `debug`
- Payload Contract:
  - Required: `trigger_event_id`, `target`, `resolved_targets`, `proposal_ids`, `status`, `memory_updated`, `concept_graph_updated`, `processed_at`
  - Optional: `error_code`, `error_detail`
- History Policy: include = no (excluded by `debug`)
- Reliability/Semantics:
  - Correlation: `trigger_event_id`

## Card: `self_improvement.proposed`
- Kind: `self_improvement.proposed`
- Layer: control
- Owner: `application/improve_approval_service`
- Producer: `propose_improvement`
- Consumers:
  - `review_improvement` (proposal lookup and validation)
- Trigger Condition: proposal creation API call
- Source/Modality: `source=system`, `modality=text`
- Tags:
  - Primary: `self_improvement.proposed`
- Payload Contract:
  - Required: `proposal_id`, `job_id`, `target`, `diff_text`, `requires_approval`, `created_by`, `created_at`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation key: `proposal_id`

## Card: `self_improvement.reviewed`
- Kind: `self_improvement.reviewed`
- Layer: control
- Owner: `application/improve_approval_service`
- Producer: `review_improvement`
- Consumers:
  - `proposal_has_review` (duplicate-review prevention)
- Trigger Condition: review API accepted and recorded
- Source/Modality: `source=system`, `modality=text`
- Tags:
  - Primary: `self_improvement.reviewed`
- Payload Contract:
  - Required: `proposal_id`, `job_id`, `target`, `decision`, `reviewed_by`, `review_reason`, `reviewed_at`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation key: `proposal_id`

## Card: `self_improvement.applied`
- Kind: `self_improvement.applied`
- Layer: control
- Owner: `application/improve_approval_service`
- Producer: `review_improvement` (approved branch apply attempt)
- Consumers:
  - operators/debug for apply result audit
- Trigger Condition: approved review leads to apply success/failure
- Source/Modality: `source=system`, `modality=text`
- Tags:
  - Primary: `self_improvement.applied`
- Payload Contract:
  - Required: `proposal_id`, `job_id`, `target`, `status`, `applied_by`, `applied_at`
  - Optional success: `applied_diff_text`
  - Optional failure: `error_code`, `error_detail`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation key: `proposal_id`

## Card: `scheduler.notice`
- Kind: `scheduler.notice`
- Layer: control
- Owner: `application/scheduler_service`
- Producer: `emit_scheduler_notice`
- Consumers:
  - `scheduler_notice_service` (turn notice into synthetic input)
- Trigger Condition: non-self-improvement schedule fire
- Source/Modality: `source=scheduler`, `modality=text`
- Tags:
  - Primary: `scheduler.notice`
- Payload Contract:
  - Required: `schedule_id`, `scheduled_at`, `noticed_at`, `action`
- History Policy: include = yes
- Reliability/Semantics:
  - Correlation key: `schedule_id` + `scheduled_at`

## Card: `scheduler.fired`
- Kind: `scheduler.fired`
- Layer: control
- Owner: `application/scheduler_service`
- Producer:
  - self-improvement path (`emit_self_improvement_event`)
  - notice path (`emit_scheduler_notice`)
- Consumers:
  - DB dedupe check (`exists_scheduler_fired`)
  - operators/debug
- Trigger Condition: schedule dispatch recorded
- Source/Modality: `source=scheduler`, `modality=text`
- Tags:
  - Primary: `scheduler.fired`
  - Secondary: action event tag (example `self_improvement.run` or `scheduler.notice`)
- Payload Contract:
  - Required: `schedule_id`, `scheduled_at`, `fired_at`, `action`
  - Optional: `disposition`
- History Policy: include = yes
- Reliability/Semantics:
  - Idempotency key: `schedule_id` + `scheduled_at`

## Card: `concept_graph.query`
- Kind: `concept_graph.query`
- Layer: observability
- Owner: `application/router_service`
- Producer: `emit_concept_graph_query_event`
- Consumers:
  - `debug_concept_graph_queries` endpoint
- Trigger Condition: router concept query processed
- Source/Modality: `source=router`, `modality=state`
- Tags:
  - Primary: `concept_graph.query`
  - Secondary: `debug`
- Payload Contract:
  - Required: `query_terms`, `limit`, `result_concepts`, `selected_seeds`, `active_concepts_from_concept_graph`
  - Optional: `error`
- History Policy: include = no (excluded by `debug`)

## Card: `llm.raw`
- Kind: `llm.raw`
- Layer: observability
- Owner: `application/execution_service`, `application/router_service`, `application/improve_service`
- Producer:
  - `emit_debug_module_events`
  - `emit_router_debug_raw`
  - `emit_trigger_debug_raw`
- Consumers:
  - debug UI / operators
- Trigger Condition: LLM call success path logging
- Source/Modality: `source=router|decision|submodule:*|self_improvement`, `modality=text`
- Tags:
  - Primary: `llm.raw`
  - Secondary: `debug`, `mode:*`, optional `module:self_improvement`
- Payload Contract:
  - Required: `raw`, `context`, `output_text`
  - Optional: `tool_calls`, `mode`, worker metadata
- History Policy: include = no (excluded by `debug`)

## Card: `llm.error`
- Kind: `llm.error`
- Layer: observability
- Owner: `application/execution_service`, `application/router_service`
- Producer:
  - `emit_debug_module_error_event`
  - `emit_router_debug_error`
- Consumers:
  - debug UI / operators
- Trigger Condition: LLM call failure path logging
- Source/Modality: `source=router|decision|submodule:*`, `modality=text`
- Tags:
  - Primary: `llm.error`
  - Secondary: `debug`, `error`, `mode:*`
- Payload Contract:
  - Required: `context`, `error`
  - Optional: `mode`
- History Policy: include = no (excluded by `debug`)

## Card: `tool` observation family (`observe`, `tool:*`, `outcome:*`)
- Kind: implicit tool-observation family
- Layer: observability
- Owner: `tools` (`StateToolHandler`)
- Producer: `emit_tool_observation`
- Consumers:
  - integration scenarios (tool log coverage metrics)
  - operators/debug
- Trigger Condition: every tool handler invocation
- Source/Modality: `source=tooling`, `modality=state`
- Tags:
  - Primary (current behavior): `observe`
  - Secondary: `tool`, `tool:<tool_name>`, `outcome:ok|error`
- Payload Contract:
  - Required: `tool_name`, `arguments`, `outcome`, `elapsed_ms`
  - Optional: `output`, `error`
- History Policy: include = no (excluded by `observe`)
- Note: this family currently has no single dedicated semantic tag such as `tool.observed`.

## Card: `error` (ingress parse error)
- Kind: `error`
- Layer: observability
- Owner: `application/debug_service`
- Producer: `parse_and_append_input` parse failure branch
- Consumers:
  - event log / diagnostics
- Trigger Condition: malformed ingress payload or invalid input type
- Source/Modality: `source=system`, `modality=text`
- Tags:
  - Primary: `error`
- Payload Contract:
  - Required: `text:string`
- History Policy: include = yes (not currently filtered unless also tagged debug/observe)

## Gaps Identified for Next Iteration
- `meta.tags` carries multiple dimensions and lacks explicit singular `kind`.
- Trigger ingress can emit arbitrary event tag names while runtime consumers are allowlist-like in practice.
- Tool observation family does not have a stable dedicated primary semantic kind.
- `error` tag is overloaded across domain/control/observability contexts.

