# ADR 0012 — Click capture allowlist

Status: Accepted

## Context

Avoid noise but keep clickable `div`s.

## Decision

Walk from target to root; record on allowlisted tags, roles and opt-in attributes.

## Consequences

`data-capture="all"` exists for sites that want everything.
