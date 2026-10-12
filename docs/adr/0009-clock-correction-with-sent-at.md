# ADR 0009 — Clock correction with sent_at

Status: Accepted

## Context

Browser clocks are often wrong.

## Decision

`ts = received_at - (sent_at - ts)`, clamped; events ordered by `seq`.

## Consequences

Events with no `sent_at` are not corrected.
