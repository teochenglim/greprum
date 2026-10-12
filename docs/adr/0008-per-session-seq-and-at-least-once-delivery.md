# ADR 0008 — Per-session seq and at-least-once delivery

Status: Accepted

## Context

The table is append-only and does not deduplicate.

## Decision

Per-session `seq` identifies events. Delivery is at-least-once; duplicates are dropped by cache and measured.

## Consequences

Several gateways can duplicate; the reconcile job reports the rate.
