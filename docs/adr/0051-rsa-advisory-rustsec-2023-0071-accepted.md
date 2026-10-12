# ADR 0051 — Accept RUSTSEC-2023-0071 (rsa, via sqlx-mysql)

Status: Superseded

## Context

`cargo audit` in CI flagged `rsa 0.9.10` (Marvin Attack, timing side channel, CVSS 5.9, **no patched
release**). `cargo tree -i rsa` shows the only path: `greprum → sqlx → sqlx-mysql → rsa`. sqlx-mysql
links the crate unconditionally for MySQL's RSA-encrypted password exchange (`sha256_password` /
`caching_sha2_password` over a non-TLS connection).

## Decision

Ignore the advisory (`.cargo/audit.toml`, `deny.toml`, the CI audit step) and record why:

- GrepRUM never performs RSA private-key operations; the client side only *encrypts* a password with the
  server's public key, and only when the server asks for that exchange. GreptimeDB's MySQL endpoint does not.
- The attack needs an attacker timing repeated RSA *decryptions* of chosen ciphertexts with a long-lived
  private key. There is no such key or oracle in this process.
- No alternative MySQL driver avoids the dependency without leaving sqlx.

## Consequences

The CI gate stays meaningful: any *other* advisory still fails the build. Revisit when (a) a patched `rsa`
ships, (b) sqlx drops or feature-gates `rsa` (check during the v0.11.0 sqlx upgrade), or (c) we add a TLS or
RSA-auth path that signs or decrypts with a private key.

## Update (v0.11.0)

sqlx 0.9 no longer depends on `rsa` (`cargo tree -i rsa` finds nothing). The sqlx upgrade removed the advisory,
so the ignore entries (`.cargo/audit.toml`, `deny.toml`, the CI audit step) were deleted and this ADR is
superseded. Any future ignore needs its own Accepted ADR (checked by `ac_0_10_6_e`).
