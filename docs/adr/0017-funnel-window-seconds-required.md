# ADR 0017 — Funnel window_seconds required

Status: Accepted

## Context

Prevents unbounded and misleading funnels.

## Decision

`window_seconds` is required; missing returns `400`.

## Consequences

Callers must choose a window.
