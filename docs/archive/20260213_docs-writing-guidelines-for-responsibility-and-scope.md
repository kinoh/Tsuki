# Docs Writing Guidelines for Responsibility and Scope

## Overview
This note records a documentation quality rule reinforced during router/concept-graph clarification work.

## Problem
Some design docs introduced terms or interfaces without sufficient context, which reduced document completeness and made responsibility boundaries harder to review.

## Decision
- Keep responsibility and interface documents strictly scoped.
- Remove out-of-scope mentions that are not defined in the same document context.
- Require explicit rationale for removal/deprecation statements.
- Treat abrupt mention of unrelated interfaces as a documentation defect.

## Why
- Responsibility documents are used as implementation contracts.
- Out-of-context terms create ambiguity and implementation drift.
- Review quality depends on whether each normative statement is self-contained and traceable.

## Applied Update
- Added a "Documentation Writing Guidelines" section to `docs/README.md`.
- The guideline emphasizes boundary clarity, scope discipline, concrete contracts, and traceable rationale.

## Follow-up
- Use these guidelines when updating any `docs/YYYYMMDD_*.md` decision record.
- During review, reject additions that introduce undefined interfaces or status without context.
