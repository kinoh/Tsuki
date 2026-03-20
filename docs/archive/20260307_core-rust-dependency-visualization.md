# Core Rust Dependency Visualization

## Overview
This document records how `core-rust` internal dependencies are visualized and why two graph variants are kept.

Compatibility Impact: None. This change only adds documentation and generated artifacts.

## Problem Statement
`core-rust` is a single binary crate, so Cargo package dependency views are too coarse to explain the internal module structure.
We needed a repeatable way to inspect module-level dependencies inside the crate without editing runtime code.

## Solution
Use `cargo-modules` against the `tsuki-core-rust` binary target, then re-render the generated DOT into a presentation-oriented DOT that keeps all nodes and edges while improving readability.

Generated artifacts:

- `docs/20260307_core-rust-module-dependencies.dot`
- `docs/20260307_core-rust-module-dependencies.svg`
- `docs/20260307_core-rust-application-dependencies.dot`
- `docs/20260307_core-rust-application-dependencies.svg`

## Design Decisions
### Keep both an overall graph and an application-focused graph
The full graph is useful for composition-root inspection around `server_app`, `tools`, `mcp`, `db`, and `scheduler`.
The application-focused graph is useful for service orchestration review inside `application/*`.

### Filter functions, traits, and types
`cargo-modules` can emit item-level nodes, but that makes this crate unreadable because `activation_concept_graph` and `server_app` dominate the graph.
We intentionally keep module-level edges only.

### Keep `uses` edges and remove `owns` edges only where needed
For dependency visualization, `uses` edges are the important contract.
`owns` edges are removed because they mostly restate the module tree and make the graph harder to scan.

### Re-render the graph instead of applying small textual patches
Simple search-and-replace on the original DOT improved direction, but the graph still carried too much visual noise:

- record-shaped nodes
- repeated `uses` edge labels
- long fully-qualified node labels
- no visual grouping beyond raw file prefixes

The final artifacts are built by parsing the `cargo-modules` output and writing a new DOT with:

- all original nodes preserved
- all original edges preserved
- shorter node labels
- cluster grouping
- left-to-right rank guidance
- invisible inter-cluster edges to stabilize layout

### Use clusters to show broad architectural zones
The full graph groups nodes into:

- `Entry`
- `Application`
- `Core`
- `Storage`
- `Integration`

The application-focused graph keeps:

- `Root`
- `Application`

This is a presentation choice only. It does not change dependency semantics.

## Reproduction
Install the tool once:

```bash
cargo install cargo-modules
```

Generate the full internal module graph:

```bash
cargo modules dependencies \
  --manifest-path core-rust/Cargo.toml \
  --bin tsuki-core-rust \
  --no-externs \
  --no-sysroot \
  --no-fns \
  --no-traits \
  --no-types \
  --no-owns \
  --layout neato \
  --splines ortho \
  > docs/20260307_core-rust-module-dependencies.dot

python3 core-rust/scripts/render_dependency_graph.py \
  --input docs/20260307_core-rust-module-dependencies.dot \
  --output docs/20260307_core-rust-module-dependencies.dot \
  --mode full

dot -Tsvg docs/20260307_core-rust-module-dependencies.dot \
  > docs/20260307_core-rust-module-dependencies.svg
```

Generate the application-focused graph:

```bash
cargo modules dependencies \
  --manifest-path core-rust/Cargo.toml \
  --bin tsuki-core-rust \
  --no-externs \
  --no-sysroot \
  --no-fns \
  --no-traits \
  --no-types \
  --no-private \
  --no-pub-crate \
  --no-pub-super \
  --no-pub-modules \
  --no-owns \
  --layout neato \
  --splines ortho \
  > docs/20260307_core-rust-application-dependencies.dot

python3 core-rust/scripts/render_dependency_graph.py \
  --input docs/20260307_core-rust-application-dependencies.dot \
  --output docs/20260307_core-rust-application-dependencies.dot \
  --mode application

dot -Tsvg docs/20260307_core-rust-application-dependencies.dot \
  > docs/20260307_core-rust-application-dependencies.svg
```

## Notes
- The graph is target-specific. It is generated for the `tsuki-core-rust` binary defined in `core-rust/src/main.rs`.
- This output shows direct Rust module usage edges, not runtime call frequency or architectural intent.
- `server_app` appears as a hub because it currently acts as the composition root and also exports shared request/response types used by `application/*`.
- The rendered DOT intentionally removes repetitive edge labels from the display, but it does not remove the edges themselves.
