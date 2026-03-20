# Event Contracts

Each event kind is defined by four axes:
- `is_context_element` — whether the event enters reasoning context (via `history_service` / `execution_service`)
- `emitters` — which module emits it
- `event_contract` — minimum shape constraints (`source`, `modality`, `tags_required`, `payload_required`)
- `receivers` — which module consumes it and how

All `is_context_element: true` events are delivered to reasoning context through
`application/history_service` and `application/execution_service`. This path is shared and not
repeated per card.

---

## Core Events

| Kind | Context | Source | Tags | Key Payload |
|---|---|---|---|---|
| `input` | ✓ | `user\|system` | `input`, `type:*` | `text` |
| `response` | ✓ | `assistant` | `response` | `text` |
| `decision` | ✓ | `decision` | `decision` | `text` |
| `submodule` | ✓ | `submodule:<name>` | `submodule` | `text` |
| `router` | ✓ | `router` | `router` | `activation_query_terms`, `hard_triggers`, `soft_recommendations` |
| `error` | ✓ | `system` | `error` | `text` |

## Scheduler Events

| Kind | Context | Source | Tags | Key Payload |
|---|---|---|---|---|
| `scheduler.fired` | ✓ | `scheduler` | `scheduler.fired` | `schedule_id`, `scheduled_at`, `fired_at`, `action` |
| `scheduler.notice` | ✓ | `scheduler` | `scheduler.notice` | `schedule_id`, `scheduled_at`, `action` |

`scheduler.fired` is also the duplicate-prevention record: the scheduler queries this event before
emitting to prevent double-firing.

## Self-Improvement Events

| Kind | Context | Source | Tags | Key Payload |
|---|---|---|---|---|
| `self_improvement.run` | ✓ | `scheduler\|system` | `self_improvement.run` | — |
| `self_improvement.module_processed` | ✓ | `self_improvement` | `self_improvement.module_processed` | `trigger_event_id`, `module_target`, `status` |
| `self_improvement.trigger_processed` | ✗ | `self_improvement` | `self_improvement.trigger_processed`, `debug` | `trigger_event_id`, `status` |
| `self_improvement.proposed` | ✓ | `system` | `self_improvement.proposed` | `proposal_id`, `job_id`, `target`, `diff_text` |
| `self_improvement.reviewed` | ✓ | `system` | `self_improvement.reviewed` | `proposal_id`, `decision` |
| `self_improvement.applied` | ✓ | `system` | `self_improvement.applied` | `proposal_id`, `status` |

## Debug / Observability Events

These events are `is_context_element: false` and are excluded from reasoning context.

| Kind | Source | Tags | Key Payload |
|---|---|---|---|
| `concept_graph.query` | `router` | `concept_graph.query`, `debug` | `query_terms`, `result_concepts` |
| `llm.raw` | `router\|decision\|submodule:*\|self_improvement` | `debug`, `llm.raw` | `raw`, `context` |
| `llm.error` | `router\|decision\|submodule:*` | `debug`, `llm.error`, `error` | `context`, `error` |
| `observe` (tool) | `tooling` | `observe`, `tool`, `tool:<name>`, `outcome:<ok\|error>` | `tool_name`, `arguments`, `outcome` |
