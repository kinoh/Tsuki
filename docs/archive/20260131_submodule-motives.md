# Decision: Submodule Motive Prompts

## Context
We want submodules that guide action selection based on three motives: curiosity, self-preservation,
and social approval. These modules should output short suggestions that the decision module can
consider alongside the event stream.

## Decision
- Replace the prior mirror/signals submodules with three motive-oriented submodules:
  - `curiosity`: maximize learning and feedback opportunities.
  - `self_preservation`: prioritize stable operation and risk reduction.
  - `social_approval`: improve perceived helpfulness and rapport.
- Each module produces a concise suggestion with a consistent format to aid decision parsing.

## Rationale
- Aligns module behavior with the conceptual goals described by the user.
- Keeps outputs interpretable while preserving the event-first architecture.

## Consequences
- Submodule count increases from two to three.
- Decision module receives broader motive coverage without changing its interface.
