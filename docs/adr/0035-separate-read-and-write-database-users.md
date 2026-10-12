# ADR 0035 — Separate read and write database users

Status: Open
Spike: spike-item-06

## Context

Spike item 6: separate GreptimeDB users with different permissions.

## Decision

Configuration supports separate `write_url` and `read_url`. Enforcing read-only permission needs GreptimeDB's static user provider with `ro`/`wo` modes and was not exercised in this spike.

## Evidence

`scripts/ro-user-test.sh` (`make ro-user-test`) starts a throwaway GreptimeDB with a static user provider (`admin` rw, `reader` ro), migrates, and asserts the read-only user can `SELECT` but not `INSERT`, `DELETE` or `DROP` (`ac_0_10_7_a`). It has not been executed yet; this ADR stays Open until it passes.

## Consequences

Until verified, both URLs may point to one user; documented as a hardening step.
