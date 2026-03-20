# RSS MCP rustls switch

## Decision
- Disabled reqwest default features and explicitly enabled rustls TLS to avoid native-tls/openssl.
- Regenerated the lockfile to drop openssl-related crates.

## Rationale
- The target is to remove OpenSSL from the dependency graph while keeping HTTPS support via rustls.
- reqwest provides a rustls feature; disabling default features prevents native-tls from being pulled in.

## Notes
- Triggered by user request to eliminate OpenSSL dependencies in `mcp/rss`.
