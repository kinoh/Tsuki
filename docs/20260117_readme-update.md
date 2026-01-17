# README Update

## Overview
Clarified the README to match the current Docker Compose layout and GUI launch options.

## Problem Statement
The README described Docker deployment as a single container and only showed the Tauri GUI command, which could mislead contributors and testers.

## Solution
Updated the Docker deployment description to reflect a multi-service Compose stack and added the Vite GUI command alongside Tauri.

## Design Decisions
- Keep the scope minimal to avoid reworking unrelated sections.
- Prefer explicit service naming to reduce confusion when bringing the stack up.

## Implementation Details
- Adjusted the Architecture bullet for Docker deployment in `README.md`.
- Expanded the GUI quick-start commands to include `npm run dev`.

## Future Considerations
- If the GUI is expected to run only via Tauri, remove the Vite command or add a short rationale.
