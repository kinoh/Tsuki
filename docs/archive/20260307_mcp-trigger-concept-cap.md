# MCP Trigger Concept Cap

## Overview
MCP tool bootstrap now caps generated trigger concepts at three entries per tool.

## Problem Statement
Trigger-concept extraction previously accepted any non-empty deduplicated list from the LLM.
This encouraged broad capability catalogs instead of a small set of high-precision triggers, especially for generic tools such as shell execution.

## Solution
- Add a hard cap of three trigger concepts per MCP tool.
- Instruct the LLM to return at most three concepts and to prefer precision over recall.
- Enforce the same cap after parsing so bootstrap behavior remains bounded even if the LLM overproduces.

## Design Decisions
- The cap is fixed in code because this is a contract choice, not a tuning knob for normal runtime operation.
- Parser-side enforcement is required because prompt-only limits are not reliable enough for bootstrap safety.

## Compatibility Impact
- Breaking by default for `core-rust` bootstrap semantics: MCP tool trigger relations may be fewer and more selective than before.
