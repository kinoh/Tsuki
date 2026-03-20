# Active Concepts And Arousal Context

## Overview
Decision and submodule input context now consumes an active concept-state summary instead of proposition text emitted from `recall_query`.

## Problem Statement
The previous shared state was labeled `active_concepts_from_concept_graph` but actually carried proposition lines such as `A evokes B	score=...`.
This leaked graph relation text into downstream prompts and duplicated capability information already provided elsewhere.

The user clarified the intended contract:
- downstream input should receive current concept-graph state
- the state should be represented as active nodes with arousal
- relation direction/type is not relevant for this context
- all node kinds should participate

## Solution
- Keep router seed selection and `recall_query` execution so current-turn activation still updates graph arousal.
- Replace the downstream shared state payload with a global active-node snapshot taken after router activation.
- Render the snapshot as `label + arousal` lines, ordered by arousal descending.
- Introduce a dedicated router config limit for this state snapshot instead of reusing query-term extraction limits.

## Design Decisions
- Input context key is `active_concepts_and_arousal`.
- The payload is a text state because downstream modules already consume prompt-ready text sections.
- The snapshot includes all graph node kinds currently materialized in the store.
- Episode nodes render with `summary` when available because opaque internal episode ids are poor prompt context.
- Capability exposure remains the responsibility of visible MCP tool contracts, not concept-graph relation text.

## Implementation Details
- Added `RouterConfig.active_state_limit`.
- Added concept-graph reader API to fetch active nodes across concepts and episodes by current arousal.
- Router still runs `recall_query` for activation side effects, then reads the active-node snapshot.
- Decision and submodule input templates now consume `{{active_concepts_and_arousal}}`.

## Compatibility Impact
- Breaking by default inside `core-rust`: router/debug payload and prompt placeholders now use `active_concepts_and_arousal`.
