# Core-Rust Event Definition Cards (Transmission Contract)

## Overview
This document defines event contracts using only four axes:
- `is_context_element`: whether the event is part of reasoning input context
- `emitters`: which module emits the event kind
- `event_contract`: minimum event shape constraints
- `receivers`: which module receives and uses the event kind

Compatibility Impact: `breaking-by-default (no compatibility layer)`.

## Contract Template
```yaml
kind: <event kind>
is_context_element: <true|false>
emitters:
  - <module>
event_contract:
  source: <constraint or "*">
  modality: <constraint or "*">
  tags_required: [<tag>, ...]
  payload_required:
    <key>: <type>
receivers:
  - <module or boundary>: <contract usage>
```

## Shared Receiver Rule for `is_context_element: true`
All `is_context_element: true` events are transmitted to reasoning context through:
- `application/history_service`
- `application/execution_service`

This shared path is omitted from each individual card to avoid repetition.

## Cards

### Card: `input`
```yaml
kind: input
is_context_element: true
emitters:
  - application/debug_service
event_contract:
  source: "user|system"
  modality: text
  tags_required: ["input", "type:*"]
  payload_required:
    text: string
receivers:
  - application/history_service: user-input detection
  - application/debug_service: debug append decision
```

### Card: `response`
```yaml
kind: response
is_context_element: true
emitters:
  - tools
event_contract:
  source: assistant
  modality: text
  tags_required: ["response"]
  payload_required:
    text: string
receivers:
  - websocket/http clients: assistant output consumption
```

### Card: `decision`
```yaml
kind: decision
is_context_element: true
emitters:
  - application/execution_service
event_contract:
  source: decision
  modality: text
  tags_required: ["decision"]
  payload_required:
    text: string
receivers:
  - application/history_service: decision detection in history pipeline
  - application/debug_service: open-turn closure check
```

### Card: `submodule`
```yaml
kind: submodule
is_context_element: true
emitters:
  - application/execution_service
event_contract:
  source: "submodule:<name>"
  modality: text
  tags_required: ["submodule"]
  payload_required:
    text: string
receivers:
  - application/history_service: submodule identity and override handling
```

### Card: `router`
```yaml
kind: router
is_context_element: true
emitters:
  - application/router_service
event_contract:
  source: router
  modality: state
  tags_required: ["router"]
  payload_required:
    activation_query_terms: array
    hard_triggers: array
    soft_recommendations: array
receivers:
  - websocket/http clients: router-state observation
```

### Card: `self_improvement.run`
```yaml
kind: self_improvement.run
is_context_element: true
emitters:
  - application/scheduler_service
  - application/trigger_ingress_api
  - application/debug_service
event_contract:
  source: "scheduler|system"
  modality: text
  tags_required: ["self_improvement.run"]
  payload_required: {}
receivers:
  - application/improve_service: trigger worker start
```

### Card: `self_improvement.module_processed`
```yaml
kind: self_improvement.module_processed
is_context_element: true
emitters:
  - application/improve_service
event_contract:
  source: self_improvement
  modality: text
  tags_required: ["self_improvement.module_processed"]
  payload_required:
    trigger_event_id: string
    module_target: string
    status: string
receivers:
  - integration harness/scenarios: self-improvement progress verification
```

### Card: `self_improvement.trigger_processed`
```yaml
kind: self_improvement.trigger_processed
is_context_element: false
emitters:
  - application/improve_service
event_contract:
  source: self_improvement
  modality: text
  tags_required: ["self_improvement.trigger_processed", "debug"]
  payload_required:
    trigger_event_id: string
    status: string
receivers:
  - integration harness/scenarios: self-improvement completion wait condition
```

### Card: `self_improvement.proposed`
```yaml
kind: self_improvement.proposed
is_context_element: true
emitters:
  - application/improve_approval_service
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
  - application/improve_approval_service: proposal validation and lookup
```

### Card: `self_improvement.reviewed`
```yaml
kind: self_improvement.reviewed
is_context_element: true
emitters:
  - application/improve_approval_service
event_contract:
  source: system
  modality: text
  tags_required: ["self_improvement.reviewed"]
  payload_required:
    proposal_id: string
    decision: string
receivers:
  - application/improve_approval_service: duplicate-review detection
```

### Card: `self_improvement.applied`
```yaml
kind: self_improvement.applied
is_context_element: true
emitters:
  - application/improve_approval_service
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
  - application/scheduler_service
event_contract:
  source: scheduler
  modality: text
  tags_required: ["scheduler.notice"]
  payload_required:
    schedule_id: string
    scheduled_at: string
    action: object
receivers:
  - application/scheduler_notice_service: transform to scheduler_notice input
```

### Card: `scheduler.fired`
```yaml
kind: scheduler.fired
is_context_element: true
emitters:
  - application/scheduler_service
event_contract:
  source: scheduler
  modality: text
  tags_required: ["scheduler.fired"]
  payload_required:
    schedule_id: string
    scheduled_at: string
    fired_at: string
receivers:
  - storage/db: duplicate-fire check by event log
```

### Card: `concept_graph.query`
```yaml
kind: concept_graph.query
is_context_element: false
emitters:
  - application/router_service
event_contract:
  source: router
  modality: state
  tags_required: ["concept_graph.query", "debug"]
  payload_required:
    query_terms: array
    result_concepts: array
receivers:
  - server/debug_api: concept-graph debug query endpoint
```

### Card: `llm.raw`
```yaml
kind: llm.raw
is_context_element: false
emitters:
  - application/execution_service
  - application/router_service
  - application/improve_service
event_contract:
  source: "router|decision|submodule:*|self_improvement"
  modality: text
  tags_required: ["debug", "llm.raw"]
  payload_required:
    raw: any
    context: string
receivers:
  - debug UI / event-log readers: LLM raw inspection
```

### Card: `llm.error`
```yaml
kind: llm.error
is_context_element: false
emitters:
  - application/execution_service
  - application/router_service
event_contract:
  source: "router|decision|submodule:*"
  modality: text
  tags_required: ["debug", "llm.error", "error"]
  payload_required:
    context: string
    error: string
receivers:
  - debug UI / event-log readers: LLM failure diagnostics
```

### Card: `observe` (tool observation family)
```yaml
kind: observe
is_context_element: false
emitters:
  - tools
event_contract:
  source: tooling
  modality: state
  tags_required: ["observe", "tool", "tool:*", "outcome:*"]
  payload_required:
    tool_name: string
    arguments: any
    outcome: string
receivers:
  - integration harness/scenarios: tool execution coverage checks
```

### Card: `error` (ingress parse error)
```yaml
kind: error
is_context_element: true
emitters:
  - application/debug_service
event_contract:
  source: system
  modality: text
  tags_required: ["error"]
  payload_required:
    text: string
receivers:
  - event-log readers: ingress parse failure diagnostics
```

