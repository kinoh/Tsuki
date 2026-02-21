# Core-Rust Breaking-Default Documentation Policy

## Context
- `core-rust` is explicitly non-deployed and requires no backward compatibility.
- Design discussions still tended to include optional migration paths or compatibility fallback language.

## Decision
- Documentation policy now enforces breaking-by-default for `core-rust`.
- Compatibility layers, dual paths, migration flags, and fallback behavior are prohibited unless explicitly justified in a dedicated decision.
- `core-rust` design/implementation docs must include a short `Compatibility Impact` statement.

## Why
- This keeps design decisions aligned with project reality.
- It removes ambiguity during review and prevents accidental over-engineering for non-required compatibility.
