# ADR 0005 — Unique counts use distinct hour tables

Status: Accepted

## Context

Flow has no `count(DISTINCT)`. Spike item 5 measures raw `count(DISTINCT)`.

## Decision

Use `GROUP BY` hour tables (`visitor_hours`, `user_hours`) and count at query time. See ADR 0041 for the flow form that works.

## Consequences

Query cost scales with visitors per window; the spike result (ADR 0035) decides whether a daily job is needed.
