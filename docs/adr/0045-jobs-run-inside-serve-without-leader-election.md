# ADR 0045 — Jobs run inside serve, without leader election

Status: Accepted

## Context

The first-seen insert (retention) and the reconcile estimate (loss, duplicates) existed as
tested SQL but nothing ran them, so retention stayed empty and the loss gauges stayed 0.
PRD §7 step 7 puts reconcile in the gateway; several gateways may share one database.

## Decision

`greprum serve` runs a scheduler (`[jobs]`, hourly by default) per configured site.
First-seen is an anti-join `INSERT … SELECT` whose timestamp is deterministic (earliest hour
window), so concurrent runs converge on one row per visitor. Reconcile only sets gauges.
No leader election. `greprum jobs run-once` runs the same code path. A failed run is counted
(`greprum_job_failures_total`) and retried on the next interval, never in a tight loop.

## Consequences

Every gateway repeats the work (cheap, idempotent). Shed counts used by reconcile are the
local process's delta since its previous run, so with several gateways the loss estimate is
conservative (it subtracts only local sheds).
