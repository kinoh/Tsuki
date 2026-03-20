---
date: 2025-12-23
---

# ADR: GUI Local Log Storage

## Context

Troubleshooting required access to recent GUI-side logs. Server upload was explicitly out of scope.

## Decision

- Logs are stored in a localStorage ring buffer (recent entries only).
- No server upload.
- Sensitive fields (`token`, `authorization`) are masked before storage.
- Log levels are color-coded in the Status overlay for quick scanning.
- A regex filter input is provided for local log search.

## Rationale

localStorage is sufficient for recent-log troubleshooting without introducing a server-side log
ingestion path. Masking sensitive fields before storage prevents credential leakage in browser
storage.
