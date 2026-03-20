---
date: 2026-02-21
---

# ADR: Breaking-by-Default Design Policy

## Context

Design discussions tended to include compatibility layers, dual paths, and fallback behavior even
though the project is not constrained by backward compatibility.

## Decision

- Compatibility layers, dual paths, migration flags, and fallback behavior are prohibited unless
  explicitly justified in a dedicated decision record.
- Every design and implementation doc must include a `Compatibility Impact` statement.

## Rationale

Removes ambiguity during review and prevents accidental over-engineering for non-required
compatibility. Keeps decisions aligned with project reality.
