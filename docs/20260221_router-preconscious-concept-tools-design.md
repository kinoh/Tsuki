# Router Preconscious Concept-Graph Tool Design

## Context
- In `core-rust`, router is the preconscious module responsible for associative filtering and information selection.
- Current operation in core already uses:
  - concept lookup by simple terms (`concept_search`)
  - associative recall from existing nodes (`recall_query`)
- This behavior should be owned by router directly, not treated as an external preprocessing step.

## Decision
- Router must receive concept-graph tools and invoke them by itself.
- Router uses both `concept_search` and `recall_query` as part of routing.
- In router flow, `concept_search` is side-effect-free lookup only for ambiguity absorption.
- `concept_search` output is used only to construct appropriate `recall_query` calls.
- Submodules and decision receive router-produced recalled nodes, and may invoke `recall_query` again when needed.

## Data Model
- No taxonomy split between "activated concepts" and "recalled facts".
- No distinction between concept and episode at runtime selection level.
- Router output should expose a single node list:
  - `router_context_nodes: Vec<String>`
- `router_context_nodes` is produced from `recall_query` results only.
- `concept_search` results are intermediate router-internal data and must not be emitted as router module output.

## Scope Constraints
- No execution policy specification is required in code for this design document.
- No additional round-limit config such as `max_router_tool_rounds` or `max_module_recall_rounds`.
- No migration plan is required.
- No backward compatibility requirement applies (`core-rust` is under development and not deployed).

## Implementation Direction
- Enable tool usage in router runtime path.
- Provide router tool handlers for `concept_search` and `recall_query`.
- Keep `concept_search` as intermediate lookup for recall seeding only.
- Keep router output centered on unified `router_context_nodes` from recall.
- Pass `router_context_nodes` to downstream modules as the shared recalled context.
