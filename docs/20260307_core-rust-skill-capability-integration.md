# Core-Rust Skill Integration Into Concept Graph Memory

> **Storage design revised.** The section below that assigns skill body storage to the state DB is
> superseded by `20260317_skill-architecture.md`. Skill bodies and auxiliary files are now owned by
> the sandbox (`shell-exec` MCP server). The concept graph, Router, and Decision responsibility
> sections remain accurate.

## Overview
This document defines how `core-rust` should introduce agent skills without turning them into another callable module or tool system.

`skill` is treated as knowledge, not as an execution unit. The concept graph acts as the index for dynamic memory, while skill bodies live in the sandbox as retrievable content. Router surfaces candidate skills for the current turn, and Decision chooses whether it needs to inspect the skill body before composing the final response or tool usage plan.

Compatibility Impact: breaking-by-default (no compatibility layer)

## Problem
`core-rust` already has two runtime concepts that may look close to skills but are not the right fit.

- `submodule` is a persistent internal reasoning module with its own prompt-shaped behavior.
- `mcp tool` is a callable capability with an explicit invocation contract.

If skills are forced into either shape, the architecture becomes muddled.

- Treating skills as more submodules would blur the boundary between internal reasoning agents and stored knowledge.
- Treating skills as callable tools would incorrectly imply that a skill performs an action instead of informing Decision.
- Treating skills as a separate side registry unrelated to the concept graph would weaken the graph-first memory design that `core-rust` is moving toward.

## Decision
Store skills directly in the concept graph as knowledge nodes.

- `skill` is a memory object, not a callable runtime unit.
- The concept graph is the primary home for skill content, relations, and embedding.
- Router is responsible for surfacing candidate skills for the current turn.
- Decision is responsible for deciding whether to inspect the full skill body.
- Skill usage is learned and improved in Decision behavior over time. The skill summary must not hardcode specific situations or execution recipes.

## Responsibility Boundaries

### Concept Graph
- Owns skill indexing, embedding, and relations.
- Treats skills as part of memory, not as external runtime plugins.
- Must remain the source of truth for skill relevance and retrieval links.

### State DB
- Owns skill body content.
- Serves as the retrievable content store for dynamic memory bodies.
- Must not take over relation or activation responsibilities from the concept graph.

### Router
- Activates and ranks candidate skills through the concept graph.
- Surfaces only lightweight skill metadata in the normal path.
- Must not inject all skill bodies into Decision context by default.
- Must not decide how a surfaced skill should be applied in the final response.

### Decision
- Receives surfaced skill summaries from Router.
- Decides whether a skill body needs to be read for the current turn.
- Decides how to combine skill knowledge with state and available tools.
- Owns the final response strategy and any learned usage policy.

### Submodules
- Remain internal reasoning modules.
- Are still separate from skill memory.
- Must not be repurposed as the generic storage or execution mechanism for skills.

### MCP Tools
- Remain callable tools.
- Are still exposed through explicit invocation contracts.
- Must not be used as the conceptual model for skills.

## Skill Model
Each skill should exist in the concept graph as an indexed memory node.

Minimum fields:

- `name`
- `name = "skill:{id}"`
- `summary`
- `body_state_key`
- `embedding`
- `updated_at`

The corresponding body is stored in the state DB under `body_state_key`.

## Why `summary` Exists
`summary` is not a replacement for the skill body.

Its role is only to let Router surface lightweight candidates and let Decision choose whether deeper reading is necessary. Because of this, `summary` must stay abstract.

`summary` must:

- identify what kind of knowledge the skill contains
- remain short and cheap to surface
- avoid prescribing concrete situations
- avoid telling Decision exactly how to apply the skill

`summary` must not:

- encode explicit scenario routing rules
- encode detailed action sequences
- substitute for the body when the body is actually needed

The full `body` remains the authoritative content.

## Skill Relations
Initial skill relation design should stay minimal.

Required relation pattern:

- trigger-like concept -> skill, using `evokes`

This is enough for the first stage because the runtime only needs to:

- activate skills from related concepts
- surface likely-relevant skill summaries
- let Decision inspect the body if needed

Do not introduce `skill -> related concept` relations by default. They add maintenance cost without a clear runtime need in the first integration step.

## Trigger Policy
Trigger associations should be represented in the concept graph itself, not duplicated in a separate manifest as runtime truth.

Policy:

- skill trigger relations live in the graph
- bootstrap may generate them similarly to MCP trigger onboarding
- the graph remains the durable representation after generation

Trigger generation should prefer concise skill-identifying material, not arbitrary long-form body text. The goal is to capture what kind of knowledge the skill contains, not to convert every phrase in the skill body into a trigger rule.

## Runtime Flow

### 1. Bootstrap / Import
- Write the skill body into the state DB.
- Write the skill node into the concept graph.
- Store the skill `summary`, `body_state_key`, and embedding on the node.
- Generate or update trigger relations from relevant concepts to the skill node.

### 2. Router
- Activate concepts from the current user input.
- Allow skill nodes to become active through graph relations and embedding-based retrieval.
- Select a small set of visible skills for the turn.
- Pass only lightweight surfaced skill information to Decision.

### 3. Decision
- Receive visible skill summaries in the decision context.
- Decide whether any surfaced skill requires deeper inspection.
- If needed, read the skill body through an internal memory lookup path.
- Use the inspected knowledge to compose the final response and any tool usage.

## Decision Access Pattern
Decision should not receive every skill body by default.

Instead:

- Router provides surfaced skill summaries.
- Decision chooses whether to inspect a skill body.
- Decision may also inspect state if the skill is relevant but current user/session state is needed for application.

This allows:

- large skills without forcing every turn to load them
- multiple surfaced skills in one turn
- learned decision-time usage policy rather than hardcoded summary-time policy

## Internal Read Path
Because skill inspection is a memory read, not an action execution, the runtime should expose an internal lookup path rather than modeling skills as callable tools.

The exact implementation can vary, but the contract should support:

- listing surfaced skills for the turn
- reading a skill body by identifier
- combining that read with existing state access paths

This is intentionally closer to memory lookup than to tool execution.

## Why Skills Are Not Submodules
The similarity is real: both may be consulted conditionally, and both may influence final behavior.

The distinction is still important.

- `submodule` produces text or reasoning as an internal module with its own prompt behavior
- `skill` provides stored knowledge that Decision reads and interprets

So the runtime may feel adjacent, but the ownership is different:

- submodule = reasoning actor
- skill = retrievable memory

That distinction should remain explicit in code and docs.

## Persistence Direction
Do not add a separate skill registry as the primary source of truth.

The concept graph should store:

- skill summary
- skill body lookup key
- skill embedding
- trigger relations

The state DB should store:

- skill body content

Any import helper, admin view, or editing interface should be secondary to the graph, not a competing runtime authority.

## Context Contract
The decision context should surface skill summaries only, for example:

```text
visible_skills:
- name: gentle_conversation_guidance
  summary: Low-pressure supportive conversational guidance
- name: playful_banter_patterns
  summary: Light playful interaction patterns
```

This surfaced view is not an instruction about how to use a skill. It only tells Decision what knowledge candidates are available to inspect.

## Rejected Alternatives

### Treat skills as callable tools
Rejected because skill inspection is memory lookup, not action execution.

### Treat skills as more submodules
Rejected because skills are knowledge objects, not additional reasoning actors.

### Store skill truth outside the concept graph
Rejected because concept graph memory is intended to carry nearly all memory forms, and skills belong inside that model.

### Encode usage situations inside summary
Rejected because it freezes Decision behavior too early and pushes learned usage policy into static metadata.

## Rollout Plan

### Phase 1
- Add skill node storage to the concept graph.
- Store skill bodies in the state DB.
- Add embedding and trigger relation support for skills.

### Phase 2
- Extend Router to surface visible skill summaries.
- Extend Decision context to receive visible skill summaries.

### Phase 3
- Add an internal skill body lookup path for Decision.
- Tune when Decision should inspect a skill body versus rely on surfaced summaries alone.

## Success Criteria
- Skills are stored in the concept graph as memory nodes.
- Router can surface visible skill summaries without loading every skill body each turn.
- Decision can choose to inspect a skill body when needed.
- Skill summaries remain abstract and do not hardcode situational usage.
- Skills do not become callable pseudo-tools or pseudo-submodules.
