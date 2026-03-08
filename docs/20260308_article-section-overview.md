# Article Section Overview

## Overview
This document captures the fixed section structure and short summaries for the article.

## Sections

### Prompt and Harness
Prompts sit at the core of personality. It is easy to write something like "make it feel like X", but that tends to flatten self-recognition and reduce plasticity. The harness changes the development cycle decisively by automatically protecting quality, much like automated tests do. Over time, conversation logs also become useful as personality regression tests.

### Concept Graph
The concept graph is valuable because it lets the system describe relations directly and handle association naturally. If human thought can be understood as activation patterns in a neural network, then those patterns should be abstractable as nodes, and actions should not be an exception. That makes it feel natural and elegant to express actions on the concept graph and connect them to dynamic loading.

### Event-Driven
Tsuki treats thought as a single serialized stream, and `core-rust` implements that model explicitly as an event stream. `Mastra`'s thread model did not fit that feeling very well, and that mismatch became one of the reasons for moving away from it.

### GUI
Using `Tauri` made it pleasantly easy to target multiple platforms. The intended feel is closer to a desktop mascot on desktop environments, and closer to a live-streaming app on mobile.

### TTS
With proper accent handling, `VOICEVOX` can reach a quality level that is not bad for Japanese speech. That is why `ja-accent` was built to improve it, but at the moment it still does not feel good enough to use constantly.

### AGENTS.md Operation
There is growing consensus that `AGENTS.md` should be reduced as much as possible. My own view is that very little information is truly necessary across every task, so the current practice is to use it mainly to close recognition gaps that surfaced during individual sessions.
