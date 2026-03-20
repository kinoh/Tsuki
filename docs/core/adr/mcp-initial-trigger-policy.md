---
date: 2026-03-07
---

# ADR: MCP Initial Trigger Concepts — Generic, Not Scenario-Specific

## Context

Bootstrap trigger generation was drifting toward scenario-specific use cases (protocol names,
content types, concrete tool chains). This front-loads use-case knowledge into bootstrap and works
against the intended learning path, where concrete trigger associations should emerge from actual
tool use and self-improvement.

## Decision

- Initial MCP trigger generation produces **generic action-category concepts** only.
- Scenario-specific examples (e.g. `curl`, `RSS`, `JSON`, `URL`, news-fetch tasks) are explicitly
  rejected during initial trigger generation.
- MCP tool descriptions remain capability-centered; downstream examples and scenario wording stay
  out of the MCP contract.
- Scenario-specific trigger expansion is deferred to learned associations after real usage.

## Rationale

Initial triggers should describe what class of action the tool represents, not what goals can be
achieved with it. Lower initial recall for specific scenarios is acceptable if it preserves cleaner
generic bootstrap semantics.

## Compatibility Impact

breaking-by-default — newly generated trigger concepts are expected to be more generic than before
