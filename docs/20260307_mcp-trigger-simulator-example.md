# MCP Trigger Simulator Example

## Overview
Added a developer-only example for MCP trigger-concept inspection.

## Problem Statement
Trigger-concept generation needs manual inspection during bootstrap tuning, but exposing that inspection flow as a runtime API would add unnecessary surface area to the production server.

## Solution
- Keep trigger simulation out of runtime HTTP/debug APIs.
- Provide a `cargo run --example` entry point instead.
- Use remote MCP tool discovery as the only source of tool specifications.

## Design Decisions
- `--url` alone lists discovered tools.
- `--url` with `--tool` runs trigger-concept extraction for that discovered tool.
- No manual fallback for description/schema input is provided, because the goal is to inspect real MCP tool contracts.
- The example reuses the same shared prompt builder and parser logic as bootstrap.

## Compatibility Impact
- No runtime compatibility impact. This is a developer workflow addition only.
