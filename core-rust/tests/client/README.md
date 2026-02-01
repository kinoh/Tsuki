# WebSocket scenario client

This client sends a fixed sequence of inputs over WebSocket and records all
server events to JSONL for manual evaluation.

## Scenario format (YAML)
Only a list of inputs is supported. Inputs are sent in order.

```yaml
inputs:
  - text: "hello"
  - text: "how are you?"
  - type: sensory
    text: "smell of rain"
  - text: "long running prompt"
    timeout_ms: 120000
```

- `type` defaults to `message` if omitted
- `timeout_ms` overrides the per-input wait time (milliseconds)

## Expected behavior
- Sends inputs in order
- Waits for at least one server message per input
- Records all received messages to a JSONL file
- Does not assert correctness; evaluation is manual
