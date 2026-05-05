# Cognitive Run Model

Status: proposal.

This document proposes a simplified replacement model for the current router / submodule /
decision module shape. It is not a description of current behavior.

## Motivation

The current module model makes the admin prompt surface hard to reason about because a single
debug run can mix several concerns:

- input parsing and event history selection
- concept activation and context construction
- prompt execution
- response choice
- tool/action execution
- event and concept graph side effects

Submodules are also not an established compatibility surface. The model can therefore be changed
without preserving the current direct submodule invocation contract.

The goal is to make each runtime turn understandable as one bounded cognitive run with explicit
inputs, intermediate outputs, and effects.

## High-Level Shape

```
external input event
  -> input frame
  -> cognition
  -> response generation
  -> response execution
  -> output events and state effects
```

Events remain the durable record of what happened, but they are not the internal communication
mechanism for every step inside a turn. Within a turn, components exchange typed data owned by the
cognitive run.

## Input Frame

An input frame is the complete set of facts provided to the cognitive run by the runtime.

It should be assembled before cognition starts and should include:

- the current external input
- selected recent event history
- recalled history, if any
- current concept/state context required by cognition
- available tools/actions and their contracts

Individual cognitive components should not independently query event history by default. Event
history selection is an orchestration-level input concern so the exact given context can be
previewed and reproduced.

## Cognition

Cognition replaces the current idea of router-as-routing. Its responsibility is to construct the
interpretive context used for response generation.

It may produce:

- interpreted input
- cognitive focus
- active concepts and arousal
- relevant recalled history
- candidate actions or response directions
- uncertainty signals
- reconsideration hints

Cognition does not produce the final user-facing response. It prepares the context and focus for
response generation.

## Response Generation

Response generation replaces the current decision role.

It consumes the input frame and cognition output, then produces a response plan rather than
directly executing effects.

Example response plan shape:

```
ResponsePlan
  kind: respond | act | reconsider | no_action
  speech
  actions
  state_effects
  concept_effects
  reason
  confidence
  missing_information
  reconsideration_targets
```

`reconsider` is a first-class outcome. A response generator may conclude that it cannot make a
good judgment with the current context and request additional analysis inside the same cognitive
run.

## Response Execution

Response execution applies the response plan to the outside world.

It owns:

- emitting user-facing response events
- executing tools/actions
- applying state effects
- applying concept graph effects
- recording execution results and failures

LLM-facing response generation should not directly mutate durable state. It should declare desired
effects, and the runtime should apply them explicitly.

## Reconsideration

Reconsideration is a graph-level control flow, not an event-driven module handoff.

A reconsideration step may run additional analysis components, rerun cognition with a different
focus, or ask response generation to evaluate a narrower candidate set. The important property is
that the reason for reconsideration and the additional inputs are visible in the run trace.

This avoids treating "I cannot judge yet" as an error and makes it a normal cognitive outcome.

## Analysis Components

The future model may contain multiple analysis components, but they do not all need to be
independent event-driven modules.

Split a component only when the split creates a useful contract:

- its input and output can be typed clearly
- it should be evaluated or tuned independently
- it may be rerun on reconsideration
- downstream consumers are meaningful and stable
- its trace is useful to operators

Keep tightly coupled reasoning together when splitting would only expose unstable intermediate
representations or increase prompt tuning surface area.

Possible components include:

- input interpretation
- focus or intent analysis
- speech motivation analysis
- concept activation
- candidate response analysis
- action planning
- response composition

These are examples, not required nodes.

## Event Boundary

Events should record durable facts and important observations:

- external input
- selected output response
- executed actions and results
- durable state or concept changes
- run trace summaries needed for later inspection

Events should not be required for every internal edge between cognitive components. Internal data
flow belongs to the cognitive run.

## Development UI Implications

The admin prompt UI should move away from arbitrary module execution and toward inspecting a
cognitive run.

Useful inspection surfaces:

- input frame preview
- cognition output
- rendered prompts and contexts per component
- response plan
- declared effects
- applied effects
- run trace and comparison between reruns

The UI should make clear whether a run is preview-only, executed without applying effects, or
executed with durable effects.

## Migration Notes

The current submodule contract does not need compatibility preservation.

A minimal migration path is:

1. Introduce an input frame builder that centralizes event history and context selection.
2. Rename or wrap router behavior as cognition output construction.
3. Change decision behavior to produce a response plan.
4. Move direct tool/action/state mutation out of response generation and into response execution.
5. Replace submodule debug execution with cognitive run inspection and component-level reruns.

The existing event stream remains useful as the durable history layer, but the proposed runtime
does not require every reasoning step to be an event-driven autonomous module.
