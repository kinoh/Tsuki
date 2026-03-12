# Sandbox Playwright CLI Runtime

## Overview

The sandbox image now includes:

- Node.js and npm
- `@playwright/cli`
- the Playwright Firefox browser bundle

This enables browser-based shell execution inside the sandbox without ad-hoc runtime installation.

## Problem Statement

Shell-based HTTP fetches are not sufficient for some public sites.
During manual validation, `curl` to `https://openai.com/news/` returned HTTP 403 with `cf-mitigated: challenge`, while browser automation using Playwright with Firefox could load the page successfully.

Installing Playwright tooling directly into a running sandbox container was also not a viable steady-state approach:

- Node.js and npm were not present
- installing Ubuntu's `npm` package pulled a very large dependency set
- `npx playwright install chrome` failed during browser package unpacking
- runtime-installed browsers landed under root-owned paths, while `shell-exec` runs as the `sandbox` user

## Solution

The image now:

1. copies Node.js tooling from an official Node image stage
2. installs `@playwright/cli` and `playwright` globally
3. installs Playwright-managed Firefox during image build with `playwright install --with-deps firefox`
4. stores browser binaries under `/ms-playwright`
5. keeps `/ms-playwright` owned by the `sandbox` user so shell-exec can launch the browser at runtime
6. fixes `/memory` ownership in the entrypoint because the mounted volume replaces the image-time directory ownership

## Design Decisions

### Use a Node image stage instead of Ubuntu's npm package

Ubuntu's `npm` package brought in a very large dependency closure during manual testing.
Copying `/usr/local` from `node:20-bookworm-slim` keeps the Dockerfile simpler and avoids using the distro `npm` meta-package as the primary Node distribution path.

### Use Playwright-managed Firefox instead of Chrome

Manual validation showed:

- `playwright-cli --browser firefox` could load `https://openai.com/news/`
- `playwright install chrome` failed during browser installation in the container

Firefox is therefore the first browser baked into the sandbox image.

### Share browser binaries through `PLAYWRIGHT_BROWSERS_PATH`

The shell-exec service runs as the `sandbox` user via the entrypoint.
If browsers are installed only under root-owned default cache directories, runtime shell-exec commands may not be able to launch them reliably.

Using `/ms-playwright` makes the browser path explicit and compatible with the runtime user.

### Fix mounted volume ownership at runtime

The sandbox uses a Docker volume at `/memory`.
That mount replaces the image filesystem entry, so build-time ownership changes do not guarantee that the runtime working directory is writable by `sandbox`.

Without a runtime `chown`, Playwright CLI could resolve its daemon session path under the `sandbox` home but still fail while spawning the daemon process from an inaccessible working directory.

The entrypoint therefore normalizes `/memory` and `/ms-playwright` ownership before dropping privileges.

## Compatibility Impact

The sandbox image becomes larger and gains browser automation capability by default.
No compatibility layer was added; the image now directly provides Playwright CLI plus Firefox as part of the runtime contract.
