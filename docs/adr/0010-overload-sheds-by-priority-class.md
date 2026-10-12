# ADR 0010 — Overload sheds by priority class

Status: Accepted

## Context

Core events should survive longest.

## Decision

Shed P2 at 50 %, P1 at 80 %; P0 only gets `503` when the spool is full.

## Consequences

Shed counts are kept by class so loss estimates stay honest.
