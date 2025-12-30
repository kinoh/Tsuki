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
