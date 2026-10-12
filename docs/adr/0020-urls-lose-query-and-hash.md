# ADR 0020 — URLs lose query and hash

Status: Accepted

## Context

Query strings often hold tokens and personal data.

## Decision

Strip query and hash; read only three `utm_` parameters on session start.

## Consequences

Campaign attribution beyond UTM needs explicit events.
