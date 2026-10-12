# ADR 0035 — Separate read and write database users

Status: Open
Spike: spike-item-06

## Context

Spike item 6: separate GreptimeDB users with different permissions.

## Decision

Configuration supports separate `write_url` and `read_url`. Enforcing read-only permission needs GreptimeDB's static user provider with `ro`/`wo` modes and was not exercised in this spike.

## Consequences

Until verified, both URLs may point to one user; documented as a hardening step.
