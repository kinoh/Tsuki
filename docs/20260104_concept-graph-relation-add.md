# Decision: Extend relation_add for Episode EVOKES

## Context
- The concept graph spec needs to allow Episode links with the same API used for Concept relations.
- The user clarified that relation_add should support Episode relations, while keeping is-a/part-of concept-only.

## Decision
- relation_add accepts Episode endpoints only for EVOKES.
- is-a and part-of remain valid only between Concepts.
- EVOKES may connect Concept->Episode, Episode->Concept, or Episode->Episode.
- relation_add strengthens EVOKES weights on repeated calls, matching Concept relations.

## Rationale
- Keeps a single relation API for Concept and Episode where it is semantically safe.
- Prevents invalid taxonomy relations involving Episodes.

## User Feedback Incorporated
- The user rejected an outdated spec that omitted weight/score considerations and required updating relation_add for Episodes.
- The user requested updating this existing log instead of creating a new file.

## Implementation Notes
- Updated `mcp/concept-graph/src/service.rs` to allow Episode endpoints for EVOKES only.
- Added tests in `mcp/concept-graph/tests/scenarios.rs` for EVOKES Episode links and is-a rejection.
