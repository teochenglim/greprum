# ADR 0039 — Noisy events in a second table

Status: Open
Spike: spike-item-10

## Context

Spike item 10: should scroll and vitals go to a second table with a shorter TTL?

## Decision

Single `events` table for v1. P2 events are shed first under load. Revisit if storage growth is dominated by P2 events.

## Consequences

No schema split yet.
