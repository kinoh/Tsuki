# Fuzzy Query Scenarios

## Overview
Added two integration scenarios:
- `core-rust/tests/integration/scenarios/fuzzy_style_name_query.yaml`
- `core-rust/tests/integration/scenarios/fuzzy_concept_intro_query.yaml`

These scenarios measure communication failures that are not primarily about factual over-explanation on tiny questions.

## Why
The target failure pattern is different from `micro_fact_question`.

Here the user is still forming the question.
The main risks are:
- premature taxonomy
- over-structured concept introductions
- clarification that feels like mechanical narrowing rather than conversation

The scenarios therefore focus on how the assistant handles ambiguity, not on whether it can produce a complete answer.

## Scenario Roles
### Fuzzy Style Name Query
- Domain: aesthetic/style naming in photography
- User intent: "I have a vibe in mind; is there a casual name for it?"
- Primary risk:
  - listing too many labels
  - forcing the vibe into a rigid terminology tree
  - ending with unnatural binary clarification

### Fuzzy Concept Intro Query
- Domain: first-contact abstract concept explanation
- User intent: "I just ran into this term; give me a foothold"
- Primary risk:
  - overloading the first answer with formalism
  - stacking jargon before the user has orientation
  - treating clarification like a search branch rather than a gentle preference check

## Design Decisions
- Scenario-only addition; no harness change.
- Metrics are framed around pacing, alignment, and natural clarification rather than raw response length.
- The concept-intro scenario keeps the concrete probe on categorical quantum mechanics because it reliably tempts the model into formal terminology too early.

## Compatibility Impact
Scenario-only addition.
No API, runner, or runtime contract change.
