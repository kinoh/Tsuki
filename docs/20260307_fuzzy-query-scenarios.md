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

## Prompt-Side Response
The scenario work exposed a model-generation-specific tendency:
- unfamiliar terms were answered with mini-lectures
- style-name questions were answered with taxonomy-first label bundles
- follow-up questions often narrowed into A/B branching instead of staying conversational

The prompt response was to add stricter response-pacing rules to the casual reply policy rather than changing runtime logic.

The added constraints explicitly prioritize:
- a 1-2 sentence foothold for unfamiliar terms and concepts
- introducing only 1-2 new concepts in one reply
- not listing labels, framework names, examples, or fine-grained distinctions unless the user asks for more
- preferring accessibility over completeness when the user says they do not understand
- using only light conversational check-ins after an explanation, not classification-style narrowing

This was intentionally treated as a prompt-level mitigation for a generation-specific communication bias, not as a domain or architecture change.

## Observed Effect
After the stronger prompt guidance, the scenarios improved in the intended direction without obvious regression in baseline small talk.

### Fuzzy Style Name Query
- The assistant stopped opening with 4-5 term bundles and usually stayed within one main label plus one backup label.
- The strongest single-run result reached:
  - `premature_taxonomy_penalty = 0.9`
  - `answer_density_fit = 0.8`
- A representative improved reply reduced the naming answer to `前景フレーミング` with `レイヤリング` as a softer alternate instead of listing `オクルージョン` and other variants up front.

### Fuzzy Concept Intro Query
- The first reply no longer front-loaded terms such as `対象/射/テンソル積/ストリング図式`.
- The strongest single-run result reached:
  - `jargon_load_control = 0.62`
  - `conceptual_pacing = 0.63`
  - `answer_density_fit = 0.6`
  - `progressive_disclosure = 0.72`
- A representative improved first reply explained categorical quantum mechanics as "thinking in terms of boxes and arrows / how things compose" and ended with a light check-in rather than a formal expansion.

### Baseline Check
- `chitchat` still passed cleanly after the stronger prompt wording.
- `shell_exec_news_fetch` still executed tools correctly; its remaining weakness was grounding on article body retrieval, which is unrelated to the fuzzy-query pacing change.

## Why This Was Kept
The stronger wording appears to target a generation-specific communication habit rather than changing the general character of the assistant.

It improved the two failure probes that motivated the work:
- concept introductions that become lectures too early
- fuzzy style questions that become taxonomy-first answers

At the same time, ordinary chitchat remained natural in the sampled runs.

## Compatibility Impact
Scenario-only addition.
No API, runner, or runtime contract change.
