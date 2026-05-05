# Thought Process Model

Status: proposal.

This document proposes a simplified replacement model for the current router / submodule /
decision module shape. It is not a description of current behavior.

## Motivation

The current module model makes the admin prompt surface hard to reason about because a single
debug run can mix several concerns:

- event selection and history formatting
- concept activation and recall
- prompt execution
- response choice
- action execution
- state and concept graph side effects

Submodules are also not an established compatibility surface. The model can therefore be changed
without preserving the current direct submodule invocation contract.

The goal is to make each turn understandable as one bounded thought process with explicit events,
decision context, selected actions, action results, and trace.

## High-Level Shape

```
events
  -> cognition
  -> decision
  -> action execution
  -> output events
```

Events are the durable record of what happened. They are also the only primary input to a thought
process. There is no separate "current external input" concept; an external input is simply
an event in the event set being considered.

Within a thought process, components may exchange typed data directly. Events are not required for
every internal edge.

## Events as Input

A thought process starts from an explicit event set.

The caller may decide which recent or selected events to provide, but it should not pre-resolve
concept graph context, recalled history, or available action choices as separate run inputs. Those
belong to cognition.

The input contract should stay narrow:

```
ThoughtProcessInput
  events
```

This keeps the run reproducible without inventing a second input channel beside the event stream.

## Cognition

Cognition constructs the decision context.

It owns interpretation of the provided events, including any access to concept graph, recall, or
state required to build the context. It also decides which actions are available to decision for
this thought process.

Cognition may perform internal state updates that belong to its own responsibility, such as
concept graph activation. Those updates are not modeled as transferable effects. In dry-run or
inspection mode, the component should expose what it would have changed as trace for humans.

Example output shape:

```
DecisionContext
  context
  available_actions
```

`context` is the cognitive context used by decision. It may include interpretation,
focus, relevant history, recalled facts, active concepts, and other compact context chosen by
cognition.

`available_actions` is the set of actions decision may choose from in this thought process.
Each available action should describe its name, when it is appropriate, and how to write its single
string input.

Cognition does not produce the final user-facing response and does not decide which external
action to execute.

## Decision

Decision chooses actions from the decision context.

It consumes the decision context and produces selected actions. It should not directly execute
external actions or mutate durable internal state.

Example output shape:

```
DecisionOutput
  actions
  reason
```

`actions` are selected from `available_actions`. An empty list means no external action should be
executed.

`reason` explains the selection for trace and operator inspection.

The decision output should stay small. It should not contain generic state effects, concept graph
effects, or execution results.

## Actions

An action is an external effect: something that affects the user, another system, or the outside
world.

Examples:

- user reply
- notification
- MCP tool call
- schedule operation
- file, network, or API interaction

An action selected by decision has a uniform shape:

```
Action
  name: string
  input: string
```

`user_reply` is an action. Its `input` is the reply text.

Actions are made available by cognition and selected by decision. Action execution
performs only the selected external actions.

Internal state changes are not actions. Concept graph activation, recall bookkeeping, local state
maintenance, and trace/debug records are component-owned behavior. They may be previewed for
humans during dry runs, but no later component should depend on receiving them as a contract.

## Action Execution

Action execution applies selected external actions.

It owns:

- validating selected actions against available executors
- executing selected external actions
- emitting resulting output events
- recording action results and failures

Action execution does not discover actions and does not decide which action should happen.

## Trace

Trace is for operators and development UI. It is not a data contract between components.

Useful trace fields include:

- component input preview
- rendered prompts and contexts
- component output
- available actions
- selected actions
- intended state changes in dry-run mode
- applied state changes in commit mode
- action results
- logs and timing

Because trace is observability data, downstream components must not rely on it for behavior.

## Analysis Components

The future model may contain multiple cognitive analysis components, but they do not all need to
be independent event-driven modules.

Split a component only when the split creates a useful contract:

- its input and output can be typed clearly
- it should be evaluated or tuned independently
- it may be rerun when additional analysis is useful
- downstream consumers are meaningful and stable
- its trace is useful to operators

Keep tightly coupled reasoning together when splitting would only expose unstable intermediate
representations or increase prompt tuning surface area.

Possible components include:

- input interpretation
- focus or intent analysis
- speech motivation analysis
- concept activation
- recall selection
- candidate response analysis
- action availability selection
- action text composition

These are examples, not required nodes.

## Event Boundary

Events should record durable facts and important observations:

- external inputs
- selected user-facing responses
- executed external actions and their results
- durable state or concept changes when they are domain facts worth recording
- run trace summaries needed for later inspection

Events should not be required for every internal edge between cognitive components. Internal data
flow belongs to the thought process.

## Development UI Implications

The admin prompt UI should move away from arbitrary module execution and toward inspecting a
thought process.

Useful inspection surfaces:

- event set preview
- cognition output
- available actions
- rendered prompts and contexts per component
- decision output
- selected actions
- action execution results
- dry-run intended changes and commit applied changes
- run trace and comparison between reruns

The UI should make clear whether a component run is preview-only, executed in dry-run mode, or
executed in commit mode. Dry-run/commit is a component execution mode for observing or applying
that component's own side effects, not a separate effect aggregation system.

## Migration Notes

The current submodule contract does not need compatibility preservation.

A minimal migration path is:

1. Introduce a thought process input that accepts an explicit event set.
2. Rename or wrap router behavior as cognition context construction.
3. Move concept graph and recall selection under cognition responsibility.
4. Change decision behavior to produce selected actions.
5. Represent user replies as actions.
6. Move direct external action execution out of decision.
7. Replace submodule debug execution with thought process inspection and component-level reruns.

The existing event stream remains useful as the durable history layer, but the proposed model does
not require every reasoning step to be an event-driven autonomous module.
