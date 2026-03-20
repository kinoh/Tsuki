# Skill Model

## Core Principle

A skill is **knowledge**, not an execution unit.

Skills are not submodules (persistent reasoning agents) and not MCP tools (callable capabilities
with invocation contracts). Treating a skill as either would blur the boundary between stored
knowledge and runtime behavior.

## Responsibility Allocation

**Concept graph** — indexes skills as knowledge nodes with embeddings and relations. Skills live
in memory, not in an external registry.

**Router** — surfaces candidate skills for the current turn via concept graph activation. Provides
only lightweight metadata (summary, tags) in the normal path. Does not inject full skill bodies
into Decision context by default.

**Decision** — receives surfaced skill summaries from Router and decides whether to read the full
skill body for the current turn. Owns the final response strategy. Skill usage is learned in
Decision behavior over time; the skill summary must not hardcode execution recipes.

**Sandbox (`shell-exec` MCP server)** — stores skill body content and auxiliary files. Is the
retrievable content store for dynamic memory bodies.

## Extension Guidance

When adding skill-related features:
- Keep skill retrieval in the Router activation path, not in Decision.
- Never add a skill invocation mechanism — if an action is needed, it belongs to MCP tools.
- Skill summaries that describe "when to use" are acceptable; step-by-step execution recipes are not.
- New skill metadata fields belong in the concept graph node, not in a separate registry.
