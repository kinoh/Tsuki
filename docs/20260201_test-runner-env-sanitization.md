# Sanitize env for test runner cargo spawns

## Context
The Rust E2E test runner spawns nested `cargo run` commands. When the runner itself
is executed via `cargo run`, Cargo injects `CARGO_MANIFEST_DIR` into its environment.
That value was then forwarded to child Cargo processes, while other invocations did
not have the variable set, causing build-script fingerprint churn and unnecessary
rebuilds.

## Decision
- Remove Cargo-injected environment variables (`CARGO_MANIFEST_DIR`, `CARGO_BIN_NAME`,
  `CARGO_CRATE_NAME`, and `CARGO_PKG_*`) when spawning child `cargo run` commands in
  the test runner.

## Rationale
- Stabilizes Cargo fingerprinting across nested runs.
- Preserves the expected "no changes, no compile" behavior when invoking the runner
  repeatedly, even when the parent process was launched via `cargo run`.
- Keeps the fix localized to the test runner without changing the build itself.
