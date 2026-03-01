# Core-Rust Event Definition Cards (Transmission Contract)

## Overview
This document defines event contracts using only four axes:
- `is_context_element`: whether the event is part of reasoning input context
- `emitters`: who can emit it
- `event_contract`: minimum event shape constraints
- `receivers`: where the event is transmitted and consumed as a contract

Compatibility Impact: `breaking-by-default (no compatibility layer)`.

## Contract Template
```yaml
kind: <event kind>
is_context_element: <true|false>
emitters:
  - <module/path>
event_contract:
  source: <constraint or "*">
  modality: <constraint or "*">
  tags_required: [<tag>, ...]
  payload_required:
    <key>: <type>
receivers:
  - <module/path>: <contract usage>
```

## Shared Receiver Rule for `is_context_element: true`
All `is_context_element: true` events are transmitted to reasoning context through:
- `application/history_service::latest_events` -> `format_event_history`
- `application/execution_service` (`run_decision`, `run_submodule_tool`, debug variants)

This shared path is omitted from each individual card to avoid repetition.

## Cards

### Card: `input`
```yaml
kind: input
is_context_element: true
emitters:
  - core-rust/src/application/debug_service.rs::parse_and_append_input
  - core-rust/src/application/debug_service.rs::maybe_append_debug_input_event
event_contract:
  source: "user|system"
  modality: text
  tags_required: ["input", "type:*"]
  payload_required:
    text: string
receivers:
  - core-rust/src/application/history_service.rs::is_user_input_event: user-input detection
  - core-rust/src/application/debug_service.rs::should_append_debug_input_for_reuse_open: debug append decision
```

### Card: `response`
```yaml
kind: response
is_context_element: true
emitters:
  - core-rust/src/tools.rs::StateToolHandler::handle_inner (emit_user_reply)
event_contract:
  source: assistant
  modality: text
  tags_required: ["response"]
  payload_required:
    text: string
receivers:
  - websocket/http event stream readers: assistant output event consumption
```

### Card: `decision`
```yaml
kind: decision
is_context_element: true
emitters:
  - core-rust/src/application/execution_service.rs::run_decision
  - core-rust/src/application/execution_service.rs::run_decision_debug
event_contract:
  source: decision
  modality: text
  tags_required: ["decision"]
  payload_required:
    text: string
receivers:
  - core-rust/src/application/history_service.rs::is_decision_event: decision-event detection
  - core-rust/src/application/debug_service.rs::should_append_debug_input_for_reuse_open: open-turn closure check
```

### Card: `submodule`
```yaml
kind: submodule
is_context_element: true
emitters:
  - core-rust/src/application/execution_service.rs::run_submodule_debug
  - core-rust/src/application/execution_service.rs::run_module (role_tag=submodule)
event_contract:
  source: "submodule:<name>"
  modality: text
  tags_required: ["submodule"]
  payload_required:
    text: string
receivers:
  - core-rust/src/application/history_service.rs::event_submodule_name: submodule identity extraction
  - core-rust/src/application/history_service.rs::apply_submodule_output_overrides: submodule output replacement
```

### Card: `router`
```yaml
kind: router
is_context_element: true
emitters:
  - core-rust/src/application/router_service.rs::run_router
event_contract:
  source: router
  modality: state
  tags_required: ["router"]
  payload_required:
    activation_query_terms: array
    hard_triggers: array
    soft_recommendations: array
receivers:
  - websocket/http event stream readers: router-state observation
```

### Card: `self_improvement.run`
```yaml
kind: self_improvement.run
is_context_element: true
emitters:
  - core-rust/src/application/scheduler_service.rs::emit_self_improvement_event
  - core-rust/src/application/trigger_ingress_api.rs::trigger_improvement
  - core-rust/src/application/debug_service.rs::parse_and_append_input (trigger type)
event_contract:
  source: "scheduler|system"
  modality: text
  tags_required: ["self_improvement.run"]
  payload_required: {}
receivers:
  - core-rust/src/application/improve_service.rs::start_trigger_consumer: trigger worker start
```

### Card: `self_improvement.module_processed`
```yaml
kind: self_improvement.module_processed
is_context_element: true
emitters:
  - core-rust/src/application/improve_service.rs::emit_module_processed_event
event_contract:
  source: self_improvement
  modality: text
  tags_required: ["self_improvement.module_processed"]
  payload_required:
    trigger_event_id: string
    module_target: string
    status: string
receivers:
  - integration scenario/harness readers: self-improvement step progress verification
```

### Card: `self_improvement.trigger_processed`
```yaml
kind: self_improvement.trigger_processed
is_context_element: false
emitters:
  - core-rust/src/application/improve_service.rs::emit_trigger_processed_event
event_contract:
  source: self_improvement
  modality: text
  tags_required: ["self_improvement.trigger_processed", "debug"]
  payload_required:
    trigger_event_id: string
    status: string
receivers:
  - integration scenario/harness readers: self-improvement completion wait condition
```

### Card: `self_improvement.proposed`
```yaml
kind: self_improvement.proposed
is_context_element: true
emitters:
  - core-rust/src/application/improve_approval_service.rs::propose_improvement
event_contract:
  source: system
  modality: text
  tags_required: ["self_improvement.proposed"]
  payload_required:
    proposal_id: string
    job_id: string
    target: string
    diff_text: string
receivers:
  - core-rust/src/application/improve_approval_service.rs::review_improvement: proposal validation and lookup
```

### Card: `self_improvement.reviewed`
```yaml
kind: self_improvement.reviewed
is_context_element: true
emitters:
  - core-rust/src/application/improve_approval_service.rs::review_improvement
event_contract:
  source: system
  modality: text
  tags_required: ["self_improvement.reviewed"]
  payload_required:
    proposal_id: string
    decision: string
receivers:
  - core-rust/src/application/improve_approval_service.rs::proposal_has_review: duplicate-review detection
```

### Card: `self_improvement.applied`
```yaml
kind: self_improvement.applied
is_context_element: true
emitters:
  - core-rust/src/application/improve_approval_service.rs::review_improvement
event_contract:
  source: system
  modality: text
  tags_required: ["self_improvement.applied"]
  payload_required:
    proposal_id: string
    status: string
receivers:
  - operations/debug readers: apply success/failure audit
```

### Card: `scheduler.notice`
```yaml
kind: scheduler.notice
is_context_element: true
emitters:
  - core-rust/src/application/scheduler_service.rs::emit_scheduler_notice
event_contract:
  source: scheduler
  modality: text
  tags_required: ["scheduler.notice"]
  payload_required:
    schedule_id: string
    scheduled_at: string
    action: object
receivers:
  - core-rust/src/application/scheduler_notice_service.rs::start_notice_consumer: transform to scheduler_notice input
```

### Card: `scheduler.fired`
```yaml
kind: scheduler.fired
is_context_element: true
emitters:
  - core-rust/src/application/scheduler_service.rs::emit_self_improvement_event
  - core-rust/src/application/scheduler_service.rs::emit_scheduler_notice
event_contract:
  source: scheduler
  modality: text
  tags_required: ["scheduler.fired"]
  payload_required:
    schedule_id: string
    scheduled_at: string
    fired_at: string
receivers:
  - core-rust/src/db.rs::exists_scheduler_fired: duplicate-fire check by event log
```

### Card: `concept_graph.query`
```yaml
kind: concept_graph.query
is_context_element: false
emitters:
  - core-rust/src/application/router_service.rs::emit_concept_graph_query_event
event_contract:
  source: router
  modality: state
  tags_required: ["concept_graph.query", "debug"]
  payload_required:
    query_terms: array
    result_concepts: array
receivers:
  - core-rust/src/main.rs::debug_concept_graph_queries: debug query API
```

### Card: `llm.raw`
```yaml
kind: llm.raw
is_context_element: false
emitters:
  - core-rust/src/application/execution_service.rs::emit_debug_module_events
  - core-rust/src/application/router_service.rs::emit_router_debug_raw
  - core-rust/src/application/improve_service.rs::emit_trigger_debug_raw
event_contract:
  source: "router|decision|submodule:*|self_improvement"
  modality: text
  tags_required: ["debug", "llm.raw"]
  payload_required:
    raw: any
    context: string
receivers:
  - debug UI / event log readers: LLM raw inspection
```

### Card: `llm.error`
```yaml
kind: llm.error
is_context_element: false
emitters:
  - core-rust/src/application/execution_service.rs::emit_debug_module_error_event
  - core-rust/src/application/router_service.rs::emit_router_debug_error
event_contract:
  source: "router|decision|submodule:*"
  modality: text
  tags_required: ["debug", "llm.error", "error"]
  payload_required:
    context: string
    error: string
receivers:
  - debug UI / event log readers: LLM failure diagnostics
```

### Card: `observe` (tool observation family)
```yaml
kind: observe
is_context_element: false
emitters:
  - core-rust/src/tools.rs::StateToolHandler::emit_tool_observation
event_contract:
  source: tooling
  modality: state
  tags_required: ["observe", "tool", "tool:*", "outcome:*"]
  payload_required:
    tool_name: string
    arguments: any
    outcome: string
receivers:
  - integration scenario/harness readers: tool execution coverage checks
```

### Card: `error` (ingress parse error)
```yaml
kind: error
is_context_element: true
emitters:
  - core-rust/src/application/debug_service.rs::parse_and_append_input (error branch)
event_contract:
  source: system
  modality: text
  tags_required: ["error"]
  payload_required:
    text: string
receivers:
  - event log readers: ingress parse failure diagnostics
```

## Note
`is_context_element` is a contract value in this document.
If runtime filtering rules change, this document and filtering implementation must be updated together.

