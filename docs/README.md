# Documentation

## Structure

```
docs/
  {area}/
    spec/      # Design model, principles, and guidance for extension — not what code does, but why
    adr/       # Architecture Decision Records: context, decision, rationale
    research/  # Evaluations, experiments, feasibility studies, and unique findings
    runbook/   # Operational procedures (infra area only)
  archive/     # Legacy flat docs — to be discarded after ADR extraction is complete
```

### Areas

| Area | Scope |
|---|---|
| `core/` | Backend runtime (router, memory, LLM, API, events, admin, debug, ...) |
| `gui/` | Tauri + Svelte client |
| `infra/` | Compose, Docker, CI/CD, Taskfile, deployment |
| `integrations/` | MCP servers, RSS, TTS, sandbox, skill packages |

## spec vs adr vs research

- **spec** — design model, principles, and guidance for extension. Not what the code does, but
  why it is shaped that way. Update in place when the model changes.
- **adr** — records *why* a decision was made: context, decision, rationale, rejected
  alternatives. Append-only; do not edit past decisions.
- **research** — evaluations, experiments, feasibility probes, and findings that informed or may
  inform design. Not decisions themselves, but the evidence behind them.

## File Naming

`{area}/{spec|adr}/{short-kebab-name}.md`

No date prefix — specs are kept current; ADRs carry their date in frontmatter.

## ADR Frontmatter

```markdown
---
date: YYYY-MM-DD
---
```
