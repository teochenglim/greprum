# ADR 0042 — TTL normalisation

Status: Accepted

## Context

`ttl = '13 months'` is stored by GreptimeDB as `1year 1month 43m 12s` (months are 30.44 days).

## Decision

Accept human units in config, validate them, and let GreptimeDB normalise.

## Consequences

Compare TTL text by presence, not equality, in tests.
