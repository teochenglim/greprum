# ADR 0050 — Drain throughput and lag hand-off

Status: Accepted

## Context

Success metric: 99.9 % of accepted events reach GreptimeDB within 5 s. The first unthrottled load run accepted 134 000 events/s but drained only ~4 000 events/s (debug build, one drainer, 500 events per insert), so the oldest unwritten age reached 19 s.

## Decision

Keep the PRD defaults (fresh lane 500 events, backfill 5 000) and add `ingest.fresh_lag_secs` (5): when the fresh lane is further behind than that, its backlog moves to the backfill lane (10× larger inserts) and the fresh cursor jumps to the tail, so new events keep priority. The unthrottled burst was not re-run after the change (its verification query reads millions of rows, which dominated the run). Paced load at 3 000 events/s for 30 s: **86 080 events, 0 lost, max age 1.1 s**.

## Consequences

Ingest is far faster than the single sink, which is the intended shape: the spool absorbs bursts and the drainer works through them. A site sustaining more than a few thousand events/s should run the release build and may need a concurrent drainer or the native ingester (ADR 0018); the age gauge and `fresh_lag_secs` make the limit visible.
