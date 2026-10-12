# ADR 0041 — Flow forms that work

Status: Accepted

## Context

`SELECT DISTINCT … GROUP BY …` is rejected as a batching flow without a time window. `EXPIRE AFTER` also means events older than the window are never rolled up.

## Decision

Distinct hour tables use `GROUP BY` with no aggregate. All flows use `EXPIRE AFTER '7 days'`; fixtures in flow tests must be recent.

## Consequences

Backfilling old data needs a one-off `INSERT … SELECT`, not the flow.
