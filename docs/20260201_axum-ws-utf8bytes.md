# Axum 0.8 WebSocket Text payload type update

## Context
- After bumping `axum` to 0.8, `Message::Text` now uses `Utf8Bytes` instead of `String`.
- `cargo check` failed in `core-rust/src/main.rs` due to type mismatches for outgoing and incoming text messages.

## Decision
- Convert outbound JSON strings with `text.into()` when constructing `Message::Text`.
- Convert inbound `Utf8Bytes` to `String` via `to_string()` before passing to `handle_input`.

## Rationale
- Matches the new Axum 0.8 API expectations while preserving existing message handling flow.
- Keeps changes minimal and localized to WebSocket send/receive paths.
