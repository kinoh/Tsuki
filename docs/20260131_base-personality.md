# Decision: Base Personality Prompt for All Modules

## Context
The Rust core runs multiple LLM-backed modules. We want a shared, consistent personality across all of them
to keep outputs aligned with the desired tone and language.

## Decision
- Introduce a shared base personality prompt (in Japanese).
- Prepend the base prompt to every module's instruction string.

## Rationale
- Ensures consistent style and communication across submodules and the decision module.
- Keeps module-specific goals intact while adding a global behavioral frame.

## Consequences
- All modules inherit the same personality, even when their roles differ.
- Updating the base prompt changes behavior system-wide.
