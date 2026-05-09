---
date: 2026-05-09
---

# ADR: Admin Cookie Secure Attribute by Runtime Configuration

## Context

The admin session cookie previously always included `Secure`. That is correct for HTTPS production
traffic, but it prevents browser sessions from working for local HTTP administration such as
`http://localhost:2953` or `http://<lan-host>:2953`.

The runtime already separates local and production behavior through `config.toml` plus the
production overlay `config.prod.toml`. Adding a separate environment variable would create another
deployment contract for behavior that belongs to server runtime configuration.

## Decision

- Add `[server].admin_cookie_secure`.
- Keep the local default `false` in `config.toml`.
- Set the production overlay to `true` in `config.prod.toml`.
- Continue using host-only cookies, `HttpOnly`, `SameSite=Strict`, `Path=/`, session persistence,
  and same-origin CSRF validation.

## Rationale

`Secure` controls transport requirements, not whether a site is trusted. Production should require
HTTPS transport for admin sessions, while local HTTP development must be able to complete the same
session flow without browser-specific exceptions.

Putting the switch in server configuration keeps the behavior explicit in the same delivery path
that already defines production runtime differences.
