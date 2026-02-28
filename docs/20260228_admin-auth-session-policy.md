# Decision: Admin Authentication and Session Policy

Date: 2026-02-28

## Overview
Define a single authentication model for human-operated admin surfaces in `core-rust`, and separate it clearly from API and WebSocket authentication paths.

## Problem Statement
- Header-based auth is operationally poor for browser-driven admin pages.
- Existing `debug` naming no longer reflects responsibility.
- Session lifetime and ownership rules were not fixed as a contract.

## Solution
- Rename human-facing operational routes from `/debug/*` to `/admin/*`.
- Authenticate `/admin/*` with cookie sessions only.
- Keep machine/API routes on header auth, and keep WebSocket auth handshake unchanged.

## Auth Boundary Contract
- `/admin/*`: cookie session auth only.
- API routes (`/events`, `/metadata`, `/config`, `/notification/*`, `/triggers`, `/proposals`, `/reviews`):
  - `authorization: <user>:<WEB_AUTH_TOKEN>` required.
- WebSocket (`/`):
  - first message auth handshake (`<user>:<WEB_AUTH_TOKEN>`) remains required.

## Session Contract
- Session cookie name: `tsuki_admin_session`.
- Session TTL: `30d` (`Max-Age=2592000`).
- Session persistence: SQLite table (`debug_sessions` currently; table rename can follow route rename in implementation step).
- `/admin/*` rejects invalid/expired sessions with `401`.
- Expired sessions are deleted when detected.

## Auth Endpoints
- `POST /auth/login`
- `POST /auth/logout`
- `GET /auth/me`

These endpoints exist to manage `/admin/*` sessions and are not used for WebSocket auth.

## Logging Requirements
- Keep HTTP access logging on all HTTP requests.
- Log auth lifecycle events explicitly:
  - login success/failure
  - logout
  - session expiration cleanup

## Naming Migration Note
- `debug` naming is deprecated for human-operated surfaces.
- New canonical term is `admin`.

## Compatibility Impact
- Breaking-by-default: no compatibility layer is required for `/debug/*`.
- Clients and tooling must move to `/admin/*`.
