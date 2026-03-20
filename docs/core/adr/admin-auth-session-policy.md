---
date: 2026-02-28
---

# ADR: Admin Authentication and Session Policy

## Context

Header-based auth is operationally poor for browser-driven admin pages. Session lifetime and
ownership rules were not fixed as a contract, and the `/debug/*` naming no longer reflected
responsibility.

## Decision

- Human-facing operational routes are at `/admin/*` (renamed from `/debug/*`).
- `/admin/*` uses cookie session auth only (no header-based auth).

### Session contract

- Cookie name: `tsuki_admin_session`
- Cookie attributes: `Secure; HttpOnly; SameSite=Strict; Path=/` (host-only, no `Domain`)
- Absolute TTL: 30 days; idle timeout: 24 hours (validated by `last_seen_at`)
- Session persistence: SQLite table (`admin_sessions`)
- Every login issues a new `session_id` (no session fixation)
- `ADMIN_AUTH_PASSWORD` change invalidates all existing sessions

### CSRF contract

All state-changing requests (`POST`, `PUT`, `PATCH`, `DELETE`) require same-origin header
validation: `Origin` must match server origin; fall back to `Referer`; otherwise reject with `403`.

### Auth endpoints

- `POST /auth/login` / `POST /auth/logout` / `GET /auth/me`

### Logging

- Log auth lifecycle events (login success/failure, logout, session expiration).
- Never log: `authorization` header values, cookie values, session IDs, raw tokens/passwords.

## Rationale

Cookie sessions are the correct model for browser-driven admin UIs. Fixed session and CSRF
contracts prevent security drift without requiring per-request review.

## Compatibility Impact

breaking-by-default — `/debug/*` has no compatibility layer
