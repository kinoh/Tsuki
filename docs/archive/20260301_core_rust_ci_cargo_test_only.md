# Core-Rust CI Scope: Cargo Test Only

## Overview
This change switches the primary non-GUI CI workflow from `core` (TypeScript) checks to `core-rust` Rust unit tests only.

Compatibility Impact: breaking-by-default (legacy `core` CI path removed from this workflow).

## Decision
- `.github/workflows/test.yml` now targets:
  - `core-rust/**`
  - `.github/workflows/test.yml`
- The job runs in `./core-rust` and executes only:
  - `cargo test`

## Why
- `core-rust` is the active backend runtime.
- The repository is moving to remove `core`, so keeping Node/pnpm checks in this workflow no longer matches ownership.
- Integration harness execution is intentionally excluded from CI because it has higher runtime/token cost and external dependencies.
- `cargo test` provides a fast and deterministic baseline gate for code changes.

## Resulting CI contract
- Pull requests that touch `core-rust` must pass Rust tests.
- Integration scenarios remain manual/explicitly triggered operational checks, not always-on CI gates.
