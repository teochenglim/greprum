# ADR 0033 — Flow result delay

Status: Accepted
Spike: spike-item-04

## Context

Spike item 4: delay from insert to a visible flow result, and whether `ADMIN FLUSH_FLOW` helps tests.

## Decision

Without a flush the first result appeared after about 6 s in a single-node v1.2.1 (batching mode). `ADMIN FLUSH_FLOW('<name>')` makes the result visible immediately and is used by every flow test.

## Consequences

Dashboards are up to ~6 s behind in the simple case; success metric (5 s to GreptimeDB) is measured at the spool, not at the rollup.
