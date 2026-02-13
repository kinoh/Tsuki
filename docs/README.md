# Documentation

This directory contains historical design decisions and detailed implementation documentation.

## Documentation Strategy

### AGENTS.md (Root Directory)
- **Purpose**: Quick reference for current codebase state
- **Audience**: AI assistant and developers needing quick orientation
- **Content**:
  - Project overview and features
  - Important development commands
  - Current architecture and file structure
  - API endpoints
- **Maintenance**: Keep synchronized with code changes

### docs/ (This Directory)
- **Purpose**: Historical design decisions and detailed implementation records
- **Audience**: Human developers seeking to understand "why" behind decisions
- **Content**:
  - Architecture design rationale and trade-offs
  - Feature implementation details
  - Historical context for major changes
  - Branch/feature-specific documentation
- **Maintenance**: Update when making significant design decisions

## File Naming Convention

Files in this directory follow the format: `YYYYMMDD_feature-name.md`

**Benefits:**
- **Chronological ordering**: Files sort naturally by creation date
- **Historical context**: Date prefix indicates when the decision was made
- **Searchability**: Easy to find documentation from specific time periods
- **Archival clarity**: Clearly identifies historical vs. current information

## When to Add Documentation

Add a new document when:
- Implementing a significant feature or architectural change
- Making design decisions that require explanation
- Completing a feature branch with notable design trade-offs
- Introducing new patterns or conventions

**Document format:**
```markdown
# Feature Name

## Overview
Brief description of what was implemented and why

## Problem Statement
What problem were we solving?

## Solution
How did we solve it?

## Design Decisions
Key architectural choices and trade-offs

## Implementation Details
Technical specifics for developers

## Future Considerations
Known limitations and potential improvements
```

## Documentation Writing Guidelines

Use the following rules to keep documents complete and reviewable.

- Define responsibility boundaries explicitly (`Router`, `Application`, `Submodule`, etc.).
- Keep statements scoped to the document purpose; avoid introducing side topics without context.
- Do not mention optional or non-adopted interfaces unless a dedicated section defines status and rationale.
- Prefer concrete contracts over vague descriptions (input/output schema, trait signatures, tool names).
- When deprecating or removing behavior, state the reason directly and concretely.
- If a document supersedes another, list the exact file names it supersedes.
- Keep "why" as first-class content: constraints, trade-offs, rejected alternatives.
- Use stable terms consistently; if a term changes, include a short migration note.
- Avoid speculative language in normative sections (`must`, `should`, `out-of-scope` are preferred).
- Ensure every normative claim can be traced to either:
  - an implemented code path, or
  - a clearly marked planned change.
