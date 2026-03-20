# Decision: Admin Authentication and Session Policy

Date: 2026-02-28

## Overview
Define a single authentication model for human-operated admin surfaces in `core-rust`.

## Problem Statement
- Header-based auth is operationally poor for browser-driven admin pages.
- Existing `debug` naming no longer reflects responsibility.
- Session lifetime and ownership rules were not fixed as a contract.

## Solution
- Rename human-facing operational routes from `/debug/*` to `/admin/*`.
- Authenticate `/admin/*` with cookie sessions only.

## Admin Route Contract
- `/admin/*`: cookie session auth only.

## Session Contract
- Session cookie name: `tsuki_admin_session`.
- Cookie attributes are fixed: `Secure; HttpOnly; SameSite=Strict; Path=/` (host-only cookie; no `Domain` attribute).
- Session lifetime:
  - absolute TTL: `30d` (`Max-Age=2592000`)
  - idle timeout: `24h` (validated by `last_seen_at`)
- Session persistence: SQLite table (`admin_sessions`).
- `/admin/*` rejects invalid/expired sessions with `401`.
- Expired sessions are deleted when detected.
- Session ID rotation and invalidation:
  - every successful login issues a new `session_id` (no session fixation).
  - logout must delete the server-side session.
  - `ADMIN_AUTH_PASSWORD` change invalidates all existing admin sessions.

## Auth Endpoints
- `POST /auth/login`
- `POST /auth/logout`
- `GET /auth/me`

These endpoints exist to manage `/admin/*` sessions.

## Credential Source
- Admin login password environment variable is `ADMIN_AUTH_PASSWORD`.
- `DEBUG_AUTH_PASSWORD` naming is not used.

## CSRF Contract
- All state-changing admin requests (`POST`, `PUT`, `PATCH`, `DELETE`) must pass CSRF validation.
- CSRF validation is same-origin header validation:
  - `Origin` must match server origin.
  - if `Origin` is absent, `Referer` must match server origin.
  - otherwise reject with `403`.
- `POST /auth/logout` is included in this rule.

## Logging Requirements
- Keep HTTP access logging on all HTTP requests.
- Log auth lifecycle events explicitly:
  - login success/failure
  - logout
  - session expiration cleanup
- Sensitive auth material must never be logged:
  - `authorization` header values
  - cookie values
  - session IDs
  - raw tokens/passwords

## Naming Migration Note
- `debug` naming is deprecated for human-operated surfaces.
- New canonical term is `admin`.

## Compatibility Impact
- Breaking-by-default for admin surface naming: no compatibility layer is required for `/debug/*`.
- Admin clients and tooling must move to `/admin/*`.
