# Cognitive Pipeline Model

## Overview

The runtime pipeline has three stages: **Router → Submodules → Decision**.
Each stage has a distinct cognitive role and must not leak into the others.

## Router (Pre-conscious Filter)

Router runs before Decision and is responsible for activation — surfacing what is relevant for the
current turn from the concept graph.

- Interprets incoming input (text, image, audio) into symbolic and embedding representations.
- Queries the concept graph for activated concepts, skills, and episodes.
- Selects hard triggers (submodules that must run) and soft recommendations.
- Emits router state and activation events.

Router does **not** decide how to respond, which tools to use, or whether to reply at all. It
provides a prepared activation snapshot for Decision to consume.

Repeated or familiar sensory inputs may still pass through and surface new facets later — strict
deduplication is intentionally avoided to preserve liveliness.

## Submodules (Motive Voices)

Submodules are persistent reasoning agents, each with a narrow motivational lens:
- `curiosity` — maximize learning and feedback opportunities.
- `self_preservation` — prioritize stable operation and risk reduction.
- `social_approval` — improve perceived helpfulness and rapport.

Each submodule produces a concise suggestion. Submodules do **not** own memory, do not decide the
final response, and do not communicate with each other directly.

Hard-triggered submodules run as part of the Router stage. Soft recommendations are passed to
Decision as advisory context.

## Decision (Integration Point)

Decision consumes the router activation snapshot and submodule suggestions alongside recent event
history to produce the final response and tool usage plan.

- Owns the respond/ignore choice.
- Owns memory (the `## Memory` section in prompts belongs to Decision only).
- Learns skill usage over time through prompt behavior.
- Does not re-run activation or trigger logic already handled by Router.

## Extension Guidance

- New reasoning concerns belong in a submodule if they are motive-shaped; in Router if they are
  activation-shaped; in Decision if they require integrating all available context.
- Do not add cross-submodule communication — motives are independent voices.
- Do not add memory ownership to Router or submodules.
- Do not duplicate activation logic in Decision; consume what Router already produced.
