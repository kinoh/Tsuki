# Embedding Model Directory Outside `/data` Volume

Compatibility Impact: breaking-by-default (default embedding model path changed from `/data/models/...` to `/opt/tsuki/models/...`; startup now fails earlier in container entrypoint when model files are missing).

## Overview
We moved the default embedding model directory for `core-rust` from `/data/models/...` to `/opt/tsuki/models/...`.

## Problem Statement
`core` mounts `core-data` to `/data` in `compose.yaml`. When model files are baked into the image under `/data/models/...`, the runtime volume mount hides image contents. This causes startup panics because the model directory cannot be found.

## Solution
- Change the default model directory constant in `core-rust` to `/opt/tsuki/models/quantized-stable-static-embedding-fast-retrieval-mrl-ja`.
- Update compose environment default to the same `/opt/tsuki/...` path.
- Add explicit entrypoint validation for required embedding files:
  - `tokenizer.json`
  - `model_rest.safetensors`
  - `embedding.q4_k_m.bin`

## Design Decisions
- Keep `/data` dedicated to mutable runtime data only.
- Use a non-mounted path (`/opt/tsuki/...`) for immutable model assets bundled with the image.
- Keep fail-fast behavior (no compatibility fallback path).

## Why This Was Chosen
- It removes path shadowing caused by volume mounts.
- It makes startup failure deterministic and easier to diagnose.
- It preserves a clean separation between immutable artifacts and runtime state.
