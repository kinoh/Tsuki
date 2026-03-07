# MCP Generic Initial Trigger Policy

## Overview
Initial MCP trigger onboarding now prefers generic action-category concepts over scenario-specific use cases.

## Problem Statement
Bootstrap trigger generation was drifting toward downstream task examples such as protocol names, content types, and concrete tool chains.
That behavior front-loads use-case knowledge into bootstrap and works against the intended learning path where concrete trigger associations should emerge from actual tool use and later self-improvement.

## Solution
- Update trigger-generation prompts to request generic action-family concepts.
- Explicitly reject use-case examples such as `curl`, `RSS`, `JSON`, `XML`, `URL`, and news-fetch tasks during initial trigger generation.
- Keep MCP tool descriptions sourced from the remote tool contract; do not patch individual tools in bootstrap code.
- Update `mcp/shell-exec` contract wording so the discovered `execute` tool description stays capability-centered instead of embedding downstream examples.

## Design Decisions
- Initial triggers should describe what class of action the tool represents, not what downstream goals can be achieved with it.
- Scenario-specific trigger expansion is intentionally deferred to learned associations after real usage.
- Lower initial recall for scenarios such as news fetching is acceptable if it preserves cleaner generic bootstrap semantics.
- Tool descriptions should explain capability, while parameter semantics stay in field descriptions and example workflows stay out of the MCP contract.

## Compatibility Impact
- Breaking by default for bootstrap semantics: newly generated MCP trigger concepts are expected to be more generic and less use-case specific than before.
