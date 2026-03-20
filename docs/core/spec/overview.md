# Core — Overview

The core is the backend runtime. It receives input, activates memory, runs reasoning modules, and
emits events. All internal coordination happens through the event stream; modules do not
communicate directly with each other.

## Functional Areas

**Event stream** — append-only log of domain facts. Every module reads from and writes to the
stream. The stream is an observability channel, not a control bus. See `module-model.md`.

**Module pipeline** — three reasoning modules operating on the event stream:
- Router: pre-conscious activation filter (input → active concept snapshot)
- Submodules: motive-based suggestion generators (curiosity, self-preservation, social approval)
- Decision: integrates all context and produces the final response

No sequential contract exists between modules. See `module-model.md`, `router-activation-model.md`.

**Concept graph** — Memgraph-backed semantic memory. Stores concepts, episodes, skills, and
relations with arousal state. Router queries it for activation; conversation recall uses a derived
vector projection in it. See `skill-model.md`, `conversation-recall-model.md`.

**Scheduler** — in-process scheduler that emits events on structured recurrence. No cron; no
separate fired-history store. See `scheduler-model.md`.

**Self-improvement** — auditable proposal/review/apply flow for prompt updates. All steps are
events. Apply is deterministic and non-LLM. See `self-improvement-model.md`.

## Key Constraints

- Event stream is the single source of truth for history and coordination.
- Modules are autonomous; adding or removing one must not require changing others.
- Configuration boundary: secrets in env vars, non-secrets in `config.toml`, no fallbacks.
- All prompt text comes from `prompts.md`; startup fails fast if any required section is missing.
- Breaking-by-default: no compatibility layers unless explicitly justified in an ADR.
