# ADR 0019 — Site key in URL, Origin allowlist, signed JWT

Status: Accepted

## Context

`sendBeacon` cannot set headers; a public key proves the site only.

## Decision

`POST /collect?k=pk_xxx`, Origin allowlist, HS256 JWT for identity.

## Consequences

Anonymous counts are indicative; signed identity protects per-user data.
