# ADR 0025 — Same-domain tracker and gateway

Status: Accepted

## Context

Ad blockers target third-party analytics domains.

## Decision

Serve tracker and gateway from the customer's domain with configurable paths.

## Consequences

Operators need a reverse proxy rule.
