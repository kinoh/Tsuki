# Metadata API Versions Null Fix

## Overview
`core-rust` returned `null` for both `/metadata.api_versions.asyncapi` and `/metadata.api_versions.openapi`.
The versions are loaded from `api-specs/*.yaml` at runtime startup and serialized into `/metadata`.

## Problem Statement
`read_spec_info_version` in `core-rust/src/server_app.rs` returns `None` when YAML parsing fails.
Both spec files contained unquoted strings that break YAML parsing in `serde_yaml`, so both version fields became `null`.

## Solution
- Quote YAML values that contain parser-sensitive characters.
  - `api-specs/asyncapi.yaml`: quote `Base64 (no data: prefix)`.
  - `api-specs/openapi.yaml`: quote `` `approved` or `rejected`. ``.
- Add a regression test in `core-rust/src/server_app.rs` to ensure both embedded spec files parse and produce `Some(version)`.

## Design Decisions
- Keep fail-fast semantics in `core-rust` logic (no fallback values were introduced).
- Fix contract sources (`api-specs/*.yaml`) instead of adding parser workarounds in application code.
- Add direct parsing coverage because this bug is caused by YAML syntax, not business logic.

Compatibility Impact: Breaking-by-default policy unchanged. No compatibility layer or fallback behavior added.

## Future Considerations
- Optional hardening: run a YAML lint/check step in CI for `api-specs/*.yaml` before merge.
