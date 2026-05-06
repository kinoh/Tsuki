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
decision context, deliberation output, selected actions, action results, and trace.

## High-Level Shape

```
events
  -> cognition
  -> deliberation contributors
  -> decision
  -> action execution
  -> output events
```

Events are the durable record of what happened. Ordered event history is the only primary input to
a thought process. There is no separate "current external input" concept; an external input is
simply an event in the history being considered.

Within a thought process, components may exchange typed data directly. Events are not required for
every internal edge.

## Event History as Input

A thought process starts from explicit ordered event history.

The caller may decide how far back to retrieve events, but it should not pre-resolve
concept graph context, recalled history, or available action choices as separate run inputs. Those
belong to cognition.

The input contract should stay narrow:

```
ThoughtProcessInput
  event_history
```

This keeps the run reproducible without inventing a second input channel beside the event stream.
Cognition derives the latest external input from the ordered event history when it needs one.

## Cognition

Cognition constructs the decision context.

It owns interpretation of the provided event history, including any access to concept graph,
recall, or state required to build the context. It also decides which actions are available to
decision for this thought process.

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

## Deliberation Contributors

Deliberation contributors produce text contributions for decision.

They are the place for former submodule-like behavior: motive lenses, risk checks, task
decomposition, focus modeling, or other bounded analyses that are useful before action choice but
should not execute external actions.

There is no separate `Deliberation` actor. Contributor execution is part of thought process
orchestration: after cognition constructs the decision context, the orchestrator runs all active
contributors against that context and passes their combined output to decision.

Contributor execution is intentionally separated from concept graph activation for now. Activation
has proven difficult to tune as an execution gate. A future router may use an LLM to decide which
contributors should run, but the current model favors predictable always-on contributors over
activation-driven firing.

Contributor output shape:

```
DeliberationContribution
  source
  text
```

`source` identifies the deliberation contributor that produced the output.

`text` is the contributor's single unstructured LLM output. It may be prose or compact notation
such as `operation=add; motive=epistemic; target=submodule; aim=clarify boundary`. The thought
process does not assign schema-level meaning to that notation.

The orchestrator aggregates contributor outputs into:

```
DeliberationOutput
  contributions
```

`DeliberationOutput` is the aggregate output of running deliberation contributors.

Existing submodules fit here when they are retained. A former submodule should no longer be treated
as an arbitrary standalone prompt or a decision-callable tool. It should instead become a
deliberation contributor that emits one text contribution according to its own explicit contract.

## Focus-Pragmatic Notation

Focus-pragmatic notation is one useful way to write `DeliberationContribution.text`. It is not a required
stage, a required top-level output shape, or a schema interpreted by the thought process.

It describes a possible intent as:

```
intent = focus operation x pragmatic motive
```

The focus operation describes how the process handles the current conceptual focus. The pragmatic
motive describes why that operation is useful in the current context. The resulting intent may be
realized by a user reply, a task action, a notification, a no-op, or another available action.

The current focus does not need to be persisted as a separate durable state. It should normally be
reconstructed from recent events, active concepts, and recalled context by cognition or by the
contributor producing the intent candidate.

Focus operations:

- `paraphrase` - express the same concept differently.
- `switch` - move attention to a related concept.
- `add` - add or update an attribute, evaluation, example, quantity, or similar information.
- `topic_shift` - move the conversation to a different topic.

Pragmatic motives:

- `affiliation` - adjust rapport, distance, face, or empathy.
- `self_interest` - manage impression, risk, or the agent's own operating conditions.
- `play` - create amusement, exaggeration, teasing, or other playful movement.
- `epistemic` - align understanding, correct recognition, or improve accuracy.
- `meta` - manage conversational progress, sequencing, or transition.

Example contribution:

```
DeliberationContribution
  source: curiosity
  text: operation=add; motive=epistemic; target=submodule; aim=clarify responsibility boundary
```

When this notation is used, the text should identify the focus operation, pragmatic motive, focus
target, and compact intent. It should not be a finished reply and should not imply that all actions
are conversational.

## Decision

Decision chooses actions from the decision context and deliberation output.

It consumes the decision context, available actions, and deliberation contribution text. It should
not directly execute external actions or mutate durable internal state.

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

`user_reply` is an action, but actions are not limited to conversation replies. The same decision
may select a reply, a task execution, a notification, a schedule operation, a concept-graph-facing
operation, or no external action.

The action `input` should be interpreted by the executor for that action. It may reference an intent
candidate when that is useful, but it is still action-specific executor input. For a
conversation-facing action, the input may be an abstract realization request rather than the final
surface text. For direct operational actions, the input may be a task description or command-like
instruction. The common contract remains a single string so decision does not need action-specific
schemas.

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
- realizing conversation-facing actions from selected intent when that responsibility belongs to
  the executor
- emitting resulting output events
- recording action results and failures

Action execution does not discover actions and does not decide which action should happen.

Execution may be simple for direct actions such as `user_reply`, but it is not limited to a
dispatcher. Complex actions may be handled by dedicated execution components that use LLMs and
tools to carry out the selected action. Decision still only selects actions; it does not execute
tools directly.

Action execution must support dry-run as an inspection mode, not as a partial external execution.
In dry-run mode it exposes the executor input that would be used in commit mode and records that in
trace, but it must not emit events, call tools, or call an LLM with tools attached. For a direct
tool action, dry-run shows the tool name and tool input. For an LLM-mediated action, dry-run shows
the LLM input and the fact that tools would only be available in commit mode. Commit mode performs
the selected action and records action results.

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
- deliberation contribution
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

The admin prompt UI should replace legacy module-specific debug runs with thought process
inspection. There is no compatibility requirement for router, decision, or submodule debug
endpoints that reconstruct or override their own run contexts outside the thought process
contract.

Useful inspection surfaces:

- ordered event history preview
- cognition output
- available actions
- rendered prompts and contexts per component
- deliberation output
- deliberation contributions
- decision output
- selected actions
- action execution results
- dry-run intended changes and commit applied changes
- run trace and comparison between reruns

The UI should make clear whether a component run is preview-only, executed in dry-run mode, or
executed in commit mode. Dry-run/commit is a component execution mode for observing or applying
that component's own side effects, not a separate effect aggregation system.

Component-level runs are allowed only when their inputs are the same contracts the component would
receive inside a thought process:

- cognition run: ordered event history
- deliberation contributor run: decision context
- decision run: decision context plus deliberation output
- action execution run: available actions plus selected actions

These runs execute one thought-process component in isolation, but they are not legacy
module-specific debug runs. Each run must use the same typed input that the component would receive
inside a thought process. Editing an intermediate input for inspection is a synthetic run and must
be labeled as such in the UI.

## Migration Notes

The current submodule contract does not need compatibility preservation.

A minimal migration path is:

1. Introduce a thought process input that accepts explicit ordered event history.
2. Rename or wrap router behavior as cognition context construction.
3. Move concept graph and recall selection under cognition responsibility.
4. Add deliberation contributors between cognition and decision.
5. Recast retained submodules as always-on deliberation contributors that produce one text
   contribution each.
6. Treat focus-pragmatic output as one possible contribution notation, not as a required
   thought-process stage or schema.
7. Change decision behavior to choose actions from decision context and deliberation output.
8. Represent user replies as actions without requiring the decision output to contain final reply
   text.
9. Move direct external action execution out of decision.
10. Replace submodule debug execution with thought process inspection and component-level runs.

The existing event stream remains useful as the durable history layer, but the proposed model does
not require every reasoning step to be an event-driven autonomous module.

## Compatibility Impact

breaking-by-default (no compatibility layer). The current submodule invocation contract, direct
reply-text decision contract, and arbitrary submodule debug execution surface are not compatibility
surfaces that must be preserved.
