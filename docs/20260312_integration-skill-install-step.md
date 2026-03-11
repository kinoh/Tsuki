# Integration Skill Install Step

## Overview

`core-rust/examples/integration_harness.rs` now supports an `install_skill` scenario step.
The step installs a state-backed skill before conversation steps begin.

## Problem Statement

Scenario tests needed a way to evaluate skill usage behavior without depending on runtime prompt edits or direct database fixtures.

The previous harness could only:

- send WebSocket conversation turns
- emit internal trigger events

That was insufficient for scenario tests that need a specific skill body and skill index metadata available before the first user turn.

## Solution

The harness now installs skills through the existing admin state-record route:

- `POST /auth/login`
- `PUT /admin/state-records/data/{key}`

The scenario step requires:

- `key`
- `body`
- `summary`
- `trigger_concepts`

The application service accepts explicit `summary` and `trigger_concepts` when `skill_index.enabled=true`.
If those fields are absent, the existing LLM-generated metadata path remains unchanged.

## Design Decisions

### Reuse the admin route instead of direct fixtures

The harness must not write directly to SQLite or Memgraph.
Skill installation belongs to application code because it updates both the state record and the activation concept graph.

Using the admin route keeps that responsibility boundary intact.

### Keep install deterministic for skill-usage scenarios

These scenario tests are intended to measure uncertainty during skill use, such as:

- whether the router surfaces the skill
- whether decision reads the body
- whether tool execution follows the skill instructions

They are not intended to measure uncertainty in metadata generation during installation.
For that reason, `install_skill` requires explicit `summary` and `trigger_concepts`.

### Preserve the production path

The admin payload extension is additive.
Normal installs can still omit explicit metadata and rely on LLM generation.

## Compatibility Impact

Breaking-by-default is preserved.
The new scenario step is additive, and the admin payload only accepts explicit metadata when `skill_index.enabled=true`.
