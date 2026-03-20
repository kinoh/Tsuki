# Submodule Concept Link Curation Script

## Context
We needed a practical way to initialize and improve submodule trigger relations without adding new runtime APIs.
The trigger policy direction is concept-graph-first, but relation quality depends on how `submodule:*` concepts are connected to existing concepts.

## Decision
Add an offline curation script:
- file: `core-rust/src/bin/link_submodule_concepts.rs`
- execution style: `cargo run --bin link_submodule_concepts -- ...`

The script:
1. Loads enabled submodules from `core-rust/config.toml`.
2. Loads candidate concepts from either:
- an explicit file (`--concepts-file`), or
- current concept graph (`debug_concept_search`).
3. Asks the model to select relevant concepts per submodule.
4. In `--apply` mode:
- ensures `submodule:<name>` concept exists (`concept_upsert`),
- adds relations from selected concepts to `submodule:<name>` (`relation_add`).
5. Defaults to dry-run unless `--apply` is explicitly set.

## Why
- Avoids introducing additional runtime ingress or API contract surface.
- Keeps curation reproducible as a script that can run repeatedly.
- Separates relation-quality work from request-time behavior.
- Preserves observability and safety by making mutation opt-in (`--apply`).

## Input/Output Rules
- Supports `--all` or repeated `--submodule <name>`.
- Supports concept input as JSON array or newline-separated text.
- Supports `--output <path>` to store dry-run/apply summaries as JSON.

## Notes
- Relation type is configurable (`evokes|is-a|part-of`), default is `evokes`.
- The script intentionally filters out `submodule:*` candidates from concept input.
- Trigger-oriented direction is `concept -> submodule`. This keeps semantics aligned with
  "front concepts activate submodules".
