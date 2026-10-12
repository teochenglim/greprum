# ADR 0029 — Delete from append-only table

Status: Accepted
Spike: spike-item-01

## Context

Spike item 1: can rows be deleted from an append-only table for erasure requests?

## Decision

No. `DELETE` fails with "DELETE is not allowed under append mode" (v1.2.1). Erasure relies on opaque ids and table `ttl`.

## Consequences

Erasure by user is not possible on `events` while `append_mode` is on; the privacy docs must say so (v1.0.0).
