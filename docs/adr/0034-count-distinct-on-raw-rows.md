# ADR 0034 — count(DISTINCT) on raw rows

Status: Accepted
Spike: spike-item-05

## Context

Spike item 5: `count(DISTINCT)` latency on raw rows. The PRD asked for 50 million; the owner descoped that run on 2026-10-12, so the evidence is the 1 M-row run.

## Decision

p95 12 ms on 1 M raw rows (ADR 0043), far inside the 10 s gate.

## Consequences

The distinct hour tables (ADR 0005) stay the query path; raw `count(DISTINCT)` is acceptable for the sizes measured. Re-run `make spike N=<events>` on production-size data before relying on it there.
