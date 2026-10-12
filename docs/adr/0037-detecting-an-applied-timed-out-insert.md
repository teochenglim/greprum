# ADR 0037 — Detecting an applied timed-out insert

Status: Accepted
Spike: spike-item-08

## Context

Spike item 8: can a timed-out insert be detected as applied, and is there idempotency over the MySQL protocol?

## Decision

No idempotency key exists and `append_mode` tables do not deduplicate. A timed-out insert is retried; if it had been applied, duplicates appear and are measured by the reconcile job via `(session_id, seq)`.

## Consequences

At-least-once with a measured duplicate rate (ADR 0008).
