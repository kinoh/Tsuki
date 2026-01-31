# Decision: Rust WebSocket Client Example

## Context
We want a Rust-native CLI client that mirrors `core/scripts/ws_client.js` to interact with the Rust core
without relying on Node.js.

## Decision
- Add `core-rust/examples/ws_client.rs` as a Rust port of the existing WS client.
- Use `tokio-tungstenite` for the WebSocket connection and `serde_json` for payload encoding.
- Keep the auth flow and message format aligned with the existing JS client.

## Rationale
- Provides a single-language workflow for Rust developers.
- Keeps the event stream inspection path identical to the JS client.
- Uses well-supported async Rust WebSocket tooling.

## Consequences
- Introduces new dependencies: `tokio-tungstenite` and `url`.
