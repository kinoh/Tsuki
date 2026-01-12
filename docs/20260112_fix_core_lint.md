# Core lint fixes

## Decisions
- Explicitly guard nullable strings to satisfy strict-boolean-expressions in admin resources.
- Convert Neo4j row values to text only when they are primitive string/number/boolean/bigint to avoid implicit object stringification.

## User feedback
- None.
