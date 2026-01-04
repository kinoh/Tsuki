# Decision: Extend relation_add for Episode EVOKES

## Context
- The concept graph spec needs to allow Episode links with the same API used for Concept relations.
- The user clarified that relation_add should support Episode relations, while keeping is-a/part-of concept-only.

## Decision
- relation_add accepts Episode endpoints only for EVOKES.
- is-a and part-of remain valid only between Concepts.
- EVOKES may connect Concept->Episode, Episode->Concept, or Episode->Episode.

## Rationale
- Keeps a single relation API for Concept and Episode where it is semantically safe.
- Prevents invalid taxonomy relations involving Episodes.

## User Feedback Incorporated
- The user rejected an outdated spec that omitted weight/score considerations and required updating relation_add for Episodes.
