# ADR 0052 — jsonwebtoken stays on 9.x

Status: Accepted

## Context

v0.11.0 tried jsonwebtoken 11. It requires choosing a crypto provider feature. The pure-Rust provider
(`rust_crypto`) pulls `rsa 0.9.10` back into the tree, reintroducing RUSTSEC-2023-0071 (no patched release)
that the sqlx 0.9 upgrade had just removed. The other provider (`aws_lc_rs`) needs a C/cmake toolchain, which
the distroless/static build and the CI images do not otherwise need. GrepRUM only verifies HS256.

## Decision

Stay on jsonwebtoken 9.3.x (HS256 pinned, no `rsa`, `cargo deny` clean). Revisit when `rsa` has a patched
release or jsonwebtoken lets HS-only users opt out of the RSA code.

## Consequences

One major (jsonwebtoken 11) is intentionally not taken. `cargo tree -i rsa` must stay empty; the audit gate
enforces it.
