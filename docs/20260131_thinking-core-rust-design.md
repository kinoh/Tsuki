# Thinking Core Rust Design

## Overview
This document captures the initial design for a Rust-based "thinking core" to replace the current core while preserving external AsyncAPI/WebSocket contracts and HTTP message history. Internally, components exchange the shared Event Format directly. The intent is to improve the conversation experience by making event-driven reasoning explicit, observable, and modular.

## Problem Statement
We need a Rust implementation of the core that:
- Supports multimodal event streams with visible history.
- Exposes an AsyncAPI/WebSocket interface to the frontend, while internally using only the shared Event Format for component-to-component communication.
- Uses OpenAI API for LLMs without locking to a fixed model or streaming.
- Allows dynamic, prompt-defined submodules with independent value functions.
- Produces actions via tool calls and feeds results back as events.
- Keeps the system understandable by reading the event log (no strong causal metadata).

## Solution
Adopt a shared Event Format and an event-store-first architecture. External integrations (e.g., frontend) speak AsyncAPI/WebSocket at the boundary, then the core translates to the Event Format and uses it internally. The system processes events in order and uses:
- Submodules (prompt-defined) to generate new events or update internal state via tools.
- A decision module to select actions by reading event history.
- Tools as the only execution surface for side effects, including response emission.

## Design Decisions
- No standalone "normalization" module. All internal components emit and consume the shared Event Format; boundary connectors translate external protocols (e.g., AsyncAPI/WebSocket) into the Event Format.
- No `ref_ids` for causality. Temporal adjacency in the event log is the primary relationship; interpretation is left to the reader.
- Submodules are prompt-defined and can be created/removed as an action requiring human approval.
- Submodule outputs are collected (wait for all with timeout) and then the decision module runs. Decision output is then broadcast to submodules.
- Decision module does not need a "round" concept. It simply queries the event store with a cutoff (latest N events or recent time window).
- Failures are first-class events.
- No security boundary for now; future tools may require human approval.

## Implementation Details

### Event Format (minimum)
All events share a common envelope. Internal events typically use text payloads.
```json
{
  "event_id": "uuid",
  "ts": "iso8601",
  "source": "user|tool|system|internal",
  "modality": "text|image|audio|state",
  "payload": { "text": "..." },
  "meta": { "tags": ["decision", "action"] }
}
```

### Event Store
- Append-only store.
- Query API for history: "latest N events" or "since timestamp".
- Minimal requirement: event stream must be visible for development.

### Submodules
- Defined purely by prompt text; can be added/removed dynamically.
- Input: new event + ability to query event history and internal state.
- Output: optional new events; tool calls for state updates.
- Value function output is natural language only (no numeric scoring).

### Internal State (Full Text Search DB)
- Schema: key, content, related_keys, metadata (updated_at).
- Read/write via tools under module control (self-directed).

### Decision Module
- Triggered after submodule outputs are collected.
- Queries event history directly (CQRS-style), rather than receiving full history in notifications.
- Emits action events; actions are executed via tool calls.
- If uncertain, emits question events directed to submodules.

### Processing Flow
1. Event arrives in common format.
2. Notify all submodules; wait for completion (timeout allowed).
3. Append submodule outputs to event store.
4. Decision module queries history and emits action events.
5. Broadcast decision events to submodules for follow-up.

## Future Considerations
- Snapshotting or summarized history to reduce query cost.
- Tool approval gate for sensitive actions.
- Parallel event processing and interruption by new events.
- Enhanced observability: tracing, decision rationale, and per-module metrics.
