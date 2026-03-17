# Agent Skill Architecture: Responsibility Boundaries and Storage Design

## Overview

This document defines the responsibility split for agent skills across the system boundary between
`core-rust` (the cognitive runtime) and `shell-exec` (the sandbox MCP server).

The central principle:

> **Sandbox is responsible for holding skill content. core-rust is responsible for knowing about it.**

This is a revision of the earlier design in `20260307_core-rust-skill-capability-integration.md`,
which assigned skill body storage to the state DB. The revised design moves body storage to the
sandbox to improve token efficiency and align with the Agent Skill specification.

---

## Agent Skill Specification

Skills follow the [Agent Skill specification](https://agentskills.io/specification).

A skill is a **directory** with the following structure:

```
{skill-key}/
  SKILL.md         # required — metadata frontmatter + skill body
  scripts/         # optional — executable helpers
  references/      # optional — supporting text files
  assets/          # optional — binary or media resources
```

### SKILL.md format

```markdown
---
name: web_page_extract
description: Extract readable text from a web page URL.
license: MIT
compatibility:
  shell: bash
  tools: [curl, node]
---

# Web Page Extract

...skill body...
```

The YAML frontmatter provides the lightweight summary (name + description, ~100 tokens).
The markdown body provides full operational detail (<5000 tokens, read on demand).
Referenced files provide supporting material (loaded only when needed).

### Progressive Disclosure

| Level | Content | When loaded |
|-------|---------|-------------|
| 1 | name + description | Always (concept graph surface) |
| 2 | SKILL.md body | When Decision chooses to inspect |
| 3 | Auxiliary files | On explicit request via `skill_read` with `path` |

This tiering means the full skill directory is never injected into the context by default.
Only what is needed for the current turn is read.

---

## Responsibility Boundaries

### Sandbox (`shell-exec` MCP server)

Owns:
- Skill file storage: `SKILL.md` and all auxiliary files under `/memory/skills/{key}/`
- Serving skill content on demand via `skill_read`
- Accepting skill installation via `skill_install`

Does not own:
- Concept graph indexing or activation
- Skill-to-concept trigger relations
- Which skills are surfaced for a given turn

### core-rust

Owns:
- Concept graph indexing: skill summaries, embeddings, trigger relations
- Routing: activating and ranking candidate skills through the graph
- Surfacing: selecting lightweight visible skills for each Decision turn
- Wiring: calling `skill_install` on sandbox during upsert, proxying `state_get` to `skill_read`

Does not own:
- The actual content of skill bodies or auxiliary files
- File paths inside the sandbox

### Why not state DB for skill bodies?

The state DB was the original skill body store. It was replaced for three reasons:

1. **Token efficiency**: State DB serves arbitrary key-value records without awareness of the
   progressive disclosure tiering that Agent Skills require. Serving a skill body from the sandbox
   keeps the read path aligned with how skills are meant to be consumed.

2. **Auxiliary files**: Agent Skills are directories, not flat strings. The state DB cannot natively
   represent a directory of files. The sandbox filesystem can.

3. **MCP boundary alignment**: The sandbox is already the runtime execution environment for skills.
   Having it also own skill content avoids cross-boundary coupling — the skill body lives next to
   the tools that the skill instructs the agent to use.

---

## Sandbox MCP Tools

### `skill_install`

```json
{
  "key": "web_page_extract",
  "files": [
    { "path": "SKILL.md", "body": "..." },
    { "path": "scripts/fetch.js", "body": "..." }
  ]
}
```

Writes the provided files into `/memory/skills/{key}/`. `SKILL.md` is required.
Key must match `[a-z0-9-]+`.

Returns:
```json
{ "ok": true, "key": "web_page_extract", "files": ["SKILL.md", "scripts/fetch.js"] }
```

### `skill_read`

```json
{ "key": "web_page_extract" }
```

or with an explicit path:

```json
{ "key": "web_page_extract", "path": "scripts/fetch.js" }
```

Defaults to `SKILL.md` when `path` is omitted.

Returns:
```json
{
  "found": true,
  "key": "web_page_extract",
  "path": "SKILL.md",
  "content": "...",
  "files": ["SKILL.md", "scripts/fetch.js"]
}
```

`files` is always the full directory listing, regardless of which file was read.

### Bootstrap exclusion

Tools whose names start with `skill_` are excluded from:
- The concept graph (no concept node, no trigger relation)
- The LLM tool list (not visible to Decision)

They are reachable only via `McpRegistry::call_tool_direct`, which bypasses `tools_by_runtime`.
This ensures skill management tooling never appears as callable actions for the agent.

---

## core-rust Integration Points

### Skill upsert → sandbox sync

When a skill is upserted with `skill_index.enabled = true`, `state_record_admin_service` calls
`skill_install` on the sandbox after updating the concept graph:

```
PUT /admin/state-records/data/{key}
  → state DB write (backward compat for list/view endpoints)
  → concept graph upsert + trigger relations
  → mcp_registry.call_tool_direct("shell_exec", "skill_install", {key, files})
```

Sandbox sync failure is non-fatal (logged, does not fail the upsert). The state DB remains the
fallback for reads until the sandbox is populated.

### `state_get` proxy

When Decision calls `state_get` with a skill key, the handler tries the sandbox first:

```
state_get({key})
  → mcp_registry.call_tool_direct("shell_exec", "skill_read", {key})
      found=true  → return sandbox response (content + files listing)
      found=false → fall through
  → state DB lookup (fallback)
```

The sandbox response format (`content`, `files`) differs from the state DB format (`record`).
Decision receives whichever is available. The concept graph surface always shows `body_state_key`
as the key to pass to `state_get`.

### `call_tool_direct`

`McpRegistry::call_tool_direct(server_id, tool_name, args)` bypasses `tools_by_runtime` and looks
up the server URL directly from `self.servers`. Used exclusively for sandbox-internal tools that
must not be exposed to the LLM.

---

## Web Page Extraction Capability

The sandbox image includes browser automation tooling to support skills that fetch web content:

- Node.js (copied from `node:20-bookworm-slim` image stage)
- `@playwright/cli` and `playwright` (globally installed, `NODE_PATH` set)
- Playwright-managed Firefox (installed at image build time under `/ms-playwright`)
- `/ms-playwright` owned by the `sandbox` user so shell-exec can launch the browser at runtime

### Why Firefox and not Chrome

Manual validation showed that `playwright-cli --browser firefox` could load `https://openai.com/news/`
(which returns HTTP 403 to `curl` via Cloudflare challenge), while `playwright install chrome` failed
during browser installation inside the container.

### Why `NODE_PATH`

`npm install -g` places packages under `/usr/local/lib/node_modules`. Without `NODE_PATH` set to
that path, `require('playwright')` from a Node script fails with module-not-found even though the
package is present globally. The Dockerfile sets `NODE_PATH=/usr/local/lib/node_modules`.

### Why not `playwright-cli` for text extraction

`playwright open` requires a display server (headed mode only). The sandbox has no X server.
Only `playwright screenshot` and `playwright pdf` work from the CLI, but neither produces text.
Text extraction requires a Node.js script using the `playwright` library directly:

```js
const { firefox } = require('playwright');
const browser = await firefox.launch();
const page = await browser.newPage();
await page.goto(process.argv[2]);
const text = await page.innerText('body');
console.log(text);
await browser.close();
```

Such scripts belong in `scripts/` within the relevant skill directory (e.g. `web_page_extract`),
not embedded in `SKILL.md` body text. This keeps the body concise and the auxiliary file reusable.

---

## Design Decisions

### Why sandbox and not a dedicated skill service

The sandbox is already the execution environment for skill-driven actions (shell commands, Node
scripts, browser automation). Placing skill content storage there avoids a third service boundary.
The MCP protocol already provides the transport; adding two tools (`skill_install`, `skill_read`)
is minimal.

### Why skill_install is a tool and not a sidecar write

If only sandbox knew where skills live, skill installation would have no callable API. Making
`skill_install` a tool means: (a) core-rust can call it via `call_tool_direct`, (b) future
scenarios or tests can install skills through the same path, and (c) the sandbox's file layout
remains an internal detail not leaked to callers.

### Why the sandbox proxy in state_get rather than a new tool

The model already knows `state_get`. Adding a new `skill_read` tool visible to the LLM would
require rewriting Decision instructions and re-educating existing behavior. Proxying inside
`state_get` is transparent: the model calls `state_get` as before; the runtime upgrades the
response when sandbox has the content.

---

## Relationship to Prior Document

`20260307_core-rust-skill-capability-integration.md` describes the original design where the
state DB owned skill body content. That document remains accurate for the concept graph and
Router/Decision responsibility sections, but its Storage section is superseded by this document:

| Component | Old | New |
|-----------|-----|-----|
| Skill body storage | state DB | sandbox `/memory/skills/{key}/` |
| Body read path | `state_get` → state DB | `state_get` → sandbox → state DB fallback |
| Auxiliary files | not supported | `skills/{key}/scripts/` etc. |
| Installation | state DB write only | state DB write + `skill_install` on sandbox |
