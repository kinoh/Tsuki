# Agent Skill Architecture: Responsibility Boundaries and Storage Design

## Overview

This document defines the responsibility split for agent skills across the system boundary between
`core-rust` (the cognitive runtime) and `shell-exec` (the sandbox MCP server).

Compatibility Impact: breaking-by-default (no compatibility layer)

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
required_mcp_tools:
  - shell_exec__execute
---

# Web Page Extract

...skill body...
```

The YAML frontmatter provides the lightweight summary (name + description, ~100 tokens) and any
Decision-visible capability requirements.
The markdown body provides full operational detail (<5000 tokens, read on demand).
Referenced files provide supporting material (loaded only when needed).

### Frontmatter Scope

`SKILL.md` frontmatter is for skill metadata that `core-rust` needs to surface without reading the
full body.

Adopted fields:

- `name`
- `description`
- `license`
- `required_mcp_tools`

`required_mcp_tools` is a list of MCP runtime tool names that must be visible to Decision when the
skill is surfaced, for example `shell_exec__execute`.

Do not use frontmatter to describe sandbox image capabilities such as installed binaries,
interpreters, or package managers. Those are properties of the sandbox platform, not per-skill
runtime metadata. A `compatibility` field is therefore out of scope for this design.

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
- Concept graph indexing: skill summaries, embeddings, trigger relations, `required_mcp_tools`
- Routing: activating and ranking candidate skills through the graph
- Surfacing: selecting lightweight visible skills for each Decision turn
- Admin API: `PUT /admin/skills/{key}` endpoint for skill installation
- Wiring: calling `skill_install` via `McpRegistry.call_tool` during skill upsert

Does not own:
- The actual content of skill bodies or auxiliary files
- File paths inside the sandbox
- Sandbox image capability declarations

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

`skill_install` is a storage contract only. It does not parse or validate `required_mcp_tools`.
`core-rust` reads skill metadata from `SKILL.md` and decides how that metadata affects Decision.

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

---

## McpRegistry and Skill Tools

### LLM tools vs core-rust callable tools

`McpRegistry` is the routing table for **all** MCP tools that core-rust can call. What the LLM
sees is a separate concern: each call site (Decision, Router, etc.) specifies its own tool list
explicitly. The registry does not dictate LLM visibility.

### Bootstrap behavior for `skill_*` tools

During bootstrap, tools whose names start with `skill_` are treated differently from regular tools:

- **Registered in `tools_by_runtime`**: yes — so they are callable via `call_tool`
- **Concept graph processing**: skipped — no concept node, no trigger concept generation, no
  activation-based surfacing

This means `skill_install` and `skill_read` are callable by core-rust but will never appear
in the concept-graph-filtered tool list passed to the LLM through the normal activation path.

### Decision LLM tool list

The Decision module explicitly adds `shell_exec__skill_read` to the tools it passes to the LLM,
independent of concept graph activation. This is intentional: skill body reading is always
available to Decision when a visible skill summary is present, without requiring the skill_read
tool itself to have a concept graph footprint.

When visible skills declare `required_mcp_tools`, Decision also receives the union of those tool
names in addition to the normal concept-graph-visible MCP tools.

`skill_install` is never included in any LLM tool list. It is only called by core-rust internals.

---

## core-rust Integration Points

### Skill admin endpoint

```
PUT /admin/skills/{key}
body: { "content": "...", "summary": "...", "trigger_concepts": [...], "files": [...] }
```

`summary` and `trigger_concepts` are optional; if omitted, they are generated via LLM.
`files` is optional and contains auxiliary skill files such as `scripts/fetch.js`.
`SKILL.md` is always derived from `content`.
`required_mcp_tools` is not supplied through the admin payload. It is parsed from `SKILL.md`
frontmatter in `content`.

The `skill_admin_service` flow is:
1. `mcp_registry.call_tool("shell_exec__skill_install", {key, files: [{path: "SKILL.md", body: content}, ...files]})`
2. Parse `required_mcp_tools` from `SKILL.md` frontmatter
3. `activation_concept_graph.skill_index_upsert(skill_name, summary, key, required_mcp_tools, true)`
4. `activation_concept_graph.skill_index_replace_triggers(skill_name, trigger_concepts)`

Skills are **not** installed through the state record endpoint (`PUT /admin/state-records/data/{key}`).
State records and skills are separate domains.

### Decision skill read flow

When Decision determines a visible skill body is needed:

```
Decision LLM calls: shell_exec__skill_read({ key: "<body_state_key from visible_skills>" })
  → sandbox returns: { found: true, content: "...", files: [...] }
```

The `body_state_key` shown in the visible_skills context is the key to pass directly to
`skill_read`. No `state_get` indirection is involved.

### Visible Skill Metadata Flow

When Router surfaces a skill, the lightweight skill metadata passed to Decision includes:

- `name`
- `summary`
- `body_state_key`
- `required_mcp_tools`

Example:

```text
visible_skills:
- name: skill:web_page_extract
  summary: Extract readable text from a web page URL.
  body_state_key: web_page_extract
  required_mcp_tools:
    - shell_exec__execute
```

Decision uses this metadata in two separate ways:

- `body_state_key` controls whether it should read the full skill body with `skill_read`
- `required_mcp_tools` controls which MCP runtime tools must be available alongside the skill

This keeps memory lookup and action capability as separate contracts.

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
`skill_install` a tool means: (a) core-rust can call it via `McpRegistry.call_tool`, (b) future
scenarios or tests can install skills through the same path, and (c) the sandbox's file layout
remains an internal detail not leaked to callers.

### Why skill_read is explicitly added to Decision tools

The LLM tool list for each module is specified at the call site, not derived from the registry.
`skill_read` is not activated by the concept graph (no concept node), so it would never appear
in the activation-filtered tool list. Decision adds it explicitly because reading a surfaced skill
body is always a legitimate action when visible skills are present, regardless of what the concept
graph has activated for the current turn.

### Why skills are not installed through state records

State records are general-purpose key-value memory. Skills are a distinct domain with their own
storage (sandbox), indexing (concept graph), and access pattern (progressive disclosure). Mixing
them couples two unrelated concerns and creates a misleading API surface.

---

## Relationship to Prior Document

`20260307_core-rust-skill-capability-integration.md` describes the original design where the
state DB owned skill body content. That document remains accurate for the concept graph and
Router/Decision responsibility sections, but its Storage section is superseded by this document:

| Component | Old | New |
|-----------|-----|-----|
| Skill body storage | state DB | sandbox `/memory/skills/{key}/` |
| Skill admin endpoint | `PUT /admin/state-records/data/{key}` | `PUT /admin/skills/{key}` |
| Body read path for Decision | `state_get` → state DB | `shell_exec__skill_read` directly |
| Auxiliary files | not supported | `skills/{key}/scripts/` etc. |
| Installation call | `call_tool_direct` (removed) | `call_tool("shell_exec__skill_install", ...)` |
