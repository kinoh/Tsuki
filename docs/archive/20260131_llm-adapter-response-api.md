# Decision: LLM Adapter for Response API

## Context
We want submodules and the decision module to call the OpenAI Response API while keeping the rest of the system
provider-agnostic. The request shape should be minimal and platform-specific settings should live at adapter creation.

## Decision
- Introduce a generic `LlmAdapter` trait with `respond(LlmRequest)` where `LlmRequest` only contains `input`.
- Add a Response API adapter using `async-openai`, configured at creation with model/instructions and optional tuning.
- Keep module prompts fixed in adapter configuration (mirror, signals, decision), and pass only runtime input.

## Rationale
- Minimizes call-site complexity and isolates platform-specific concerns in adapter setup.
- Makes it easy to swap providers by implementing the same adapter trait.
- Keeps the event stream consistent and observable by emitting request/response events per module.

## Consequences
- Requires `OPENAI_API_KEY` to run the Rust core with real calls.
- Module behavior depends on adapter configuration rather than per-request parameters.
