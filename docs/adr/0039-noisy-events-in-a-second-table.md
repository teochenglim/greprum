# ADR 0039 — Noisy events in a second table

Status: Accepted
Spike: spike-item-10

## Context

Spike item 10: should scroll and vitals go to a second table with a shorter TTL?

## Decision

Single `events` table for v1. In the 1 M-event synthetic run P2 events (scroll, vitals, pageleave) are 22.5 % and P1 (rage, dead, error) 11.3 %: not dominant, and P2 is already the first class shed under load.

## Consequences

Revisit with real traffic: if P2 exceeds ~60 % of stored rows, add a second table with a shorter TTL behind the `Sink` trait without touching the gateway.
