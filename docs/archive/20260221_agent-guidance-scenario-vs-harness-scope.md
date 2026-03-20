# Agent Guidance: Scenario-vs-Harness Scope Separation

## Context
- During integration-test discussions, scenario metric changes and harness behavior changes were unintentionally mixed.
- This caused scope drift and made collaboration less predictable.

## Decision
- Added an explicit scope-separation rule to agent guidance:
  - Scenario updates are limited to scenario specification files.
  - Test mechanism updates are limited to harness/runner implementation files.
- Cross-scope edits now require explicit user confirmation.

## Why
- Scenario iteration should remain lightweight and focused on evaluation intent.
- Harness changes affect runtime behavior and should be handled as a separate engineering decision.
- Separating scopes reduces accidental overreach and improves review clarity.
