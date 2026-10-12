# ADR 0013 — Rage and dead click rules

Status: Accepted

## Context

Defaults match PostHog's documented behaviour and limit false positives.

## Decision

Rage: 3 clicks, 30 px, 1000 ms with ignore rules. Dead: no DOM change, scroll, selection or URL change in 1 s; links never count.

## Consequences

Thresholds are configurable.
