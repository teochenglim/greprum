# ADR 0038 — fsync interval and dedup window

Status: Open
Spike: spike-item-09

## Context

Spike item 9: is a 100 ms fsync interval acceptable and is a 10 minute dedup cache enough?

## Decision

Defaults kept: 100 ms / 1 MB and 10 minutes. Property and fault tests pass at these values; production load numbers are pending.

## Consequences

Revisit after the v0.4.0 load test.
