# ADR 0048 — kill -9 results

Status: Accepted

## Context

PRD §7: a gateway crash loses no acknowledged event. Earlier tests simulated a crash by dropping objects in-process.

## Decision

`tests/ac_ops.rs` spawns the real binary, ingests, sends SIGKILL at random points (during ingest, during drain, 20 seeded rounds with restarts on the same spool) and checks the database. Result on v1.2.1: **2 168 acknowledged events, 0 lost**; 2 396 rows stored, so **228 duplicates (9.5 %)**. In the drain-kill case 500 rows were duplicated: the batch written just before the checkpoint was saved is replayed.

## Consequences

At-least-once is proven with the real process. Duplicates are bounded by one fresh batch (500 events) per kill and are exactly what the reconcile job's duplicate rate reports. Shrinking the window would need an idempotent sink (not available, ADR 0037).
