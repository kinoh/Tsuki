# Decision Context Primary History

Compatibility Impact: breaking-by-default (no compatibility layer)

## Overview

This document records the decision to redefine the decision-module input context so that recent visible conversation history is the primary context and every other section is explicitly supplemental.

## Problem Statement

- The previous `input.decision_context_template` used flat fact-style section labels only.
- `recent_event_history` and `recalled_event_history` were both injected as similarly shaped conversation blocks without explicit authority boundaries.
- The layout placed long recalled text near the end of the prompt without clear block termination or a strong statement that it was supplemental.
- User feedback identified the practical failure mode directly:
  - `recent_event_history` is the essential source for conversational flow coherence.
  - `latest_user_input` is important, but it is still derivable from recent history.
  - The template should avoid synthetic priority attributes and custom bracket syntax.
  - XML tags are preferable because boundaries and roles are clearer.

## Decision

- `recent_event_history` becomes the first and primary decision-context block.
- All other decision-context sections are nested under a single `<supplemental_context>` block.
- The decision context uses XML-style tags instead of ad-hoc bracket markers.
- No synthetic priority attribute layer is introduced.
- `recalled_event_history` includes an explicit constraint that it must only be used when consistent with `recent_event_history`.
- `output_contract` remains explicit and uses its own closing tag so the prompt has a clear terminal contract.

## Rationale

- Recent visible history is the canonical conversation-flow signal for the next reply.
- The latest user utterance is still valuable as a focused restatement, but it should not outrank the conversation flow that contains it.
- Grouping non-primary inputs under `supplemental_context` reduces ambiguity about authority without forcing numeric ranking metadata into the prompt.
- XML tags make boundaries legible to both operators and the model and reduce the chance that recalled wording is treated as continuation of the main dialogue stream.

## Implementation Details

- Updated `core-rust/config.toml` `input.decision_context_template` to:
  - place `<recent_event_history>` first
  - move all other blocks under `<supplemental_context>`
  - add an explicit consistency rule inside `<recalled_event_history>`
  - keep `<output_contract>` as the final explicit block

## Rejected Alternatives

- Keeping flat section labels only
  - rejected because it leaves primary and supplemental context semantically too close.
- Introducing `priority=*` attributes
  - rejected because the extra abstraction does not clarify authority as well as explicit primary vs. supplemental structure.
- Wrapping the whole prompt in `DECISION_CONTEXT_BEGIN/END`
  - rejected because the outer wrapper adds noise without clarifying section responsibility.
