# Documentation

## Structure

```
docs/
  {area}/
    spec/     # Design model, principles, and guidance for extension — not what code does, but why
    adr/      # Architecture Decision Records: context, decision, rationale
    runbook/  # Operational procedures (infra area only)
  archive/    # Legacy flat docs — to be discarded after ADR extraction is complete
```

### Areas

| Area | Scope |
|---|---|
| `core/` | Backend runtime (router, memory, LLM, API, events, admin, debug, ...) |
| `gui/` | Tauri + Svelte client |
| `infra/` | Compose, Docker, CI/CD, Taskfile, deployment |
| `integrations/` | MCP servers, RSS, TTS, sandbox, skill packages |

## spec vs adr

- **spec** — describes how things work *now*: interface contracts, configuration references,
  runbooks. Update in place when the implementation changes.
- **adr** — records *why* a decision was made: context, decision, rationale, rejected
  alternatives. Append-only; do not edit past decisions.

## File Naming

`{area}/{spec|adr}/{short-kebab-name}.md`

No date prefix — specs are kept current; ADRs carry their date in frontmatter.

## ADR Frontmatter

```markdown
---
date: YYYY-MM-DD
---
```
