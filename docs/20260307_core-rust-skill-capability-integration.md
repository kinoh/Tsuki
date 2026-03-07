# Core-Rust Skill Capability Integration

## Overview
This document defines how `core-rust` should introduce agent skills that are discovered and exposed dynamically through concept activation, in the same operational style as MCP tools.

The goal is not to rename existing submodules into skills. The goal is to add a new local capability type that participates in the same visibility pipeline as MCP tools while preserving the existing role of submodules as persistent internal cognitive modules.

Compatibility Impact: breaking-by-default (no compatibility layer)

## Problem
`core-rust` currently has two adjacent but different execution models.

- Submodules are persistent internal modules. They are activated by the concept graph and can be hard-triggered in Router, but Decision still receives a callable tool for every active submodule name.
- MCP tools are bootstrapped into a registry, linked to trigger concepts, and exposed to Decision only when they become visible for the current turn.

This creates a gap for agent skills.

- If skills are implemented as more submodules, the distinction between persistent inner modules and situational abilities collapses.
- If skills are implemented as ad-hoc local tools without concept-driven visibility, they bypass the router-first activation model.
- If skills are implemented with separate loading and visibility logic from MCP tools, the runtime will gain another parallel orchestration path without a clear responsibility boundary.

## Decision
Introduce a shared `Capability` layer and model local skills as one capability kind beside MCP tools.

- Keep `submodule` as a separate domain concept.
  - `submodule` means a persistent internal module such as curiosity or self-preservation.
  - `skill` means a situational callable ability selected for the current turn.
- Add a runtime abstraction that covers both dynamically exposed MCP tools and dynamically exposed local skills.
- Router remains responsible for activation-driven visibility.
- Decision receives only visible capabilities for the current turn.
- Skill prompt bodies and heavy definitions must be loaded lazily only when the skill is actually executed.

## Responsibility Boundaries

### Router
- Owns capability visibility selection through concept activation.
- Must not read full skill prompt bodies in the normal activation path.
- Must not execute local skills unless a future design explicitly marks a skill as hard-triggerable.

### Decision
- Consumes visible capabilities prepared by Router.
- May call a visible capability if it directly serves the user request.
- Must not re-score or re-rank skill relevance outside Router output.

### Capability Registry
- Owns descriptor discovery, validation, and current-turn exposure data.
- Unifies the lookup path for `mcp_tool` and `local_skill`.
- Must expose stable runtime tool contracts to Decision.

### Skill Registry
- Owns local skill manifests and lazy loading of skill definitions.
- Must not own cross-turn relevance policy.
- Must not duplicate Router visibility logic.

### Submodules
- Remain persistent internal modules.
- Continue to be activated through `submodule:{name}` concepts.
- Must not become the generic container for all callable skills.

## Capability Model
The runtime should introduce a shared descriptor model similar to:

```text
CapabilityDescriptor
- runtime_name
- kind: mcp_tool | local_skill
- concept_key
- description
- input_schema
- llm_parameters
- execution_mode: decision_tool | hard_triggerable
```

Each capability descriptor must be light enough to load at startup.

The runtime should also define an execution interface similar to:

```text
CapabilityExecutor
- validate_call_arguments(runtime_name, args)
- execute(runtime_name, args, context)
```

This separates three concerns cleanly:

- discovery/bootstrap
- visibility resolution
- execution

## Skill Data Model
Skills should be split into two layers.

### Skill Manifest
Startup-safe metadata:

- `name`
- `description`
- `input_schema`
- `trigger_concepts` or `trigger_source`
- `execution_mode`
- `prompt_path`
- optional allowed tools policy

### Skill Definition
Loaded only at execution time:

- full skill prompt
- optional execution instructions
- optional tool allowlist

This prevents the normal routing path from loading all skill prompts every turn.

## Trigger and Concept Strategy
Skills should use the same concept-graph association style as MCP tools.

- Create a concept node `skill:{name}` for each skill.
- Add `evokes` relations from trigger concepts to `skill:{name}`.
- Resolve visibility from concept activation on the `skill:{name}` node.

Trigger source policy:

1. Prefer explicit `manifest.trigger_concepts`.
2. If missing, derive trigger concepts from `description` and `input_schema`.
3. Do not derive trigger concepts from the full prompt body in the default bootstrap path.

Why:

- Skill prompts are usually longer and noisier than MCP tool descriptions.
- Trigger extraction from full prompts will pull in persona wording and examples that should not become activation concepts.
- Precision matters more than recall in router-driven visibility.

## Runtime Flow

### Bootstrap
- Load MCP tool descriptors.
- Load skill manifests.
- Register both as capabilities.
- Upsert capability concept nodes and trigger relations into the concept graph.

### Router
- Resolve active concepts from the current user input.
- Activate related targets through the concept graph.
- Compute visibility for both MCP tools and local skills.
- Return a single capability visibility payload in router output.

### Decision
- Receive only visible capabilities.
- Expose them as callable tools together with always-on base tools.
- Execute a capability through the shared dispatcher when selected.

### Skill Execution
- When a visible skill is called, load its full definition lazily.
- Run the skill through a dedicated local execution path.
- Emit normal runtime and debug events under explicit ownership such as `skill:{name}`.

## Required Refactoring Direction

### 1. Generalize visible MCP tools into visible capabilities
Current router output contains MCP-specific fields. This should become capability-oriented output.

Examples:

- `mcp_visible_tools` -> `visible_capability_names`
- `mcp_tool_visibility` -> `capability_visibility`

### 2. Introduce a shared capability dispatcher
Current decision handling branches between built-in tools, MCP tools, and submodule tools. Local skills should not add a fourth unrelated branch.

The runtime should centralize callable capability execution behind one dispatcher.

### 3. Keep submodule execution separate
`run_submodule__{name}` remains a submodule-specific tool path. It should not be merged blindly into local skills because the execution semantics differ.

The difference is intentional:

- submodule: internal reasoning module
- skill: situational callable ability

### 4. Generalize concept-driven activation helpers carefully
Current concept-graph helpers are submodule-specific. They may be generalized into target activation helpers if needed, but the API should keep target-type ownership explicit.

Avoid a vague "activate everything related" contract.

## Execution Policy
Initial rollout should support only `decision_tool` skills.

Why:

- It matches the current MCP tool exposure pattern.
- It avoids introducing another hard-trigger execution path before the capability abstraction is stable.
- It preserves the current meaning of submodule hard triggers.

Hard-triggerable skills may be added later, but only after a new design decision defines:

- allowed skill categories
- ownership of pre-decision execution
- observability and failure handling rules

## Persistence and Configuration
Skills should not reuse the `modules` table.

Recommended separation:

- `modules`: persistent internal submodules
- `skills`: local callable capability manifests

Why:

- Their lifecycles are different.
- Their execution policies are different.
- Their concepts in the runtime architecture are different.

Configuration should provide:

- skill manifest root path
- optional enable/disable flags per skill
- optional bootstrap policy settings for trigger derivation

Prompt wording must remain outside Rust source and stay in prompt/config-owned files.

## Rejected Alternatives

### Treat all skills as submodules
Rejected because it blurs the line between persistent internal modules and situational abilities, and it keeps Decision exposure too broad.

### Treat skills as built-in always-on tools
Rejected because it bypasses concept-driven visibility and weakens the router-first design.

### Add a separate skill visibility pipeline unrelated to MCP tooling
Rejected because it creates duplicated runtime policy and another parallel orchestration path.

## Rollout Plan

### Phase 1
- Add capability abstractions and keep existing MCP behavior unchanged behind the new interface.
- Introduce `SkillManifest` loading without executing skills yet.

### Phase 2
- Add skill concept bootstrap and visibility resolution.
- Expose visible skills to Decision as callable tools.

### Phase 3
- Refine debug/event ownership and optional operator controls.
- Evaluate whether selected skills need a later hard-trigger policy.

## Success Criteria
- Router determines visibility for both MCP tools and local skills through one capability-oriented model.
- Decision sees only visible skills for the current turn.
- Skill prompt bodies are not loaded in the normal routing path.
- Submodules keep their current architectural meaning.
- No new hidden fallback path is introduced for missing skill metadata or invalid execution contracts.
