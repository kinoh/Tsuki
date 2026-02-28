# Core-Rust Config Merge Tool for Production Build

## Overview
Replaced Dockerfile `sed`-based config mutation with an explicit TOML merge tool (`merge_toml`) built from repository code.

## Problem Statement
- Container build previously rewrote `db.path` using `sed`.
- This was fragile, implicit, and difficult to reason about when config structure changed.
- The desired model is base config + minimal production overlay with deterministic merge rules.

## Solution
- Added `core-rust/src/bin/merge_toml.rs`.
- Added `core-rust/config.base.toml` and `core-rust/config.prod.toml`.
- Updated `docker/core-rust/Dockerfile` to:
  - build `merge_toml` and `tsuki-core-rust`
  - generate `/tmp/config.toml` via merge
  - copy generated config into runtime image
  - remove `sed` path rewrite entirely

## Merge Contract
- Overlay keys must already exist in base (base acts as schema).
- Table values: deep merge.
- Scalar values: replace.
- Array values: replace.
- Type mismatch: error.

## CLI Contract
`merge_toml --base <path> --overlay <path> [--overlay <path> ...] --output <path> [--check]`

## Compatibility Impact
- No compatibility layer added.
- Build process changed to fail-fast when overlay introduces unknown keys or type mismatches.
