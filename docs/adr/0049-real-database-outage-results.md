# ADR 0049 — Real database outage results

Status: Accepted

## Context

PRD §7: GreptimeDB down or slow ⇒ the spool grows, then sheds by class, then 503; a backlog drains after recovery. Acceptance asks for a 10 minute outage.

## Decision

`make outage-test` pauses the GreptimeDB container for 600 s while the real gateway ingests ~20 batches/s. Result: **12 200 batches, all `204`; 36 600 events acknowledged, 36 600 stored, 0 lost, 0 duplicates**; oldest unwritten event age peaked at 669 s during the outage and returned to 0 after the backlog drained. A first run exposed a harness/config issue: 41 % of requests got `429` because the default per-key rate limit was 600 requests/min; the default is now 60 000/min.

## Consequences

No `503` occurs while the spool is below its 2 GB cap. The rate limit is an abuse guard and must be sized for the busiest legitimate site; operators should monitor `greprum_rejected_by_total{reason="rate_limit"}`.
