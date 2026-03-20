# Prompt Directory Memory

## Overview
Replace structured-memory prompt injection with a filesystem-backed prompt directory under `/work/prompts` to make updates simpler and more inspectable.

## Problem Statement
The structured-memory MCP interface is difficult for the LLM to update directly, and prompt memory content needs to be managed in a clearer, file-based way.

## Solution
Load prompt memory from a directory and inject it into the agent prompt as a concatenated stream of Markdown files.

## Design Decisions
- Read from a fixed directory `/work/prompts` mounted by Compose for consistent availability.
- Include only `.md` files and recurse into subdirectories.
- Sort by relative path (lexicographic order) for stable prompt composition.
- Cap each file at 4KB per user direction; if exceeded, truncate and insert an explicit warning line in the prompt content.
- Format each section as `# <relative/path>` followed by content, then `---` to delimit sections.
- Use the shell-exec MCP server to read prompt files because `/work` lives in the sandbox container.

## Implementation Details
- `ActiveUser.loadMemory()` now uses the shell-exec MCP server instead of calling `structured-memory`.
- The loader lists prompt files via shell commands, reads each file with a size header, applies truncation, and assembles the final prompt string.
- Read errors are logged and skipped to avoid blocking the response flow.

## Future Considerations
- Make the prompt directory configurable if deployment environments diverge.
- Consider total prompt size limits across all files to avoid oversized memory injection.
