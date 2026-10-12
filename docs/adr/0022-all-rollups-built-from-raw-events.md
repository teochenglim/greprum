# ADR 0022 — All rollups built from raw events

Status: Accepted

## Context

Chaining flows was unconfirmed. Spike item 3 confirmed that a flow can read another flow's sink in v1.2.1 (ADR 0031).

## Decision

Keep every rollup built from raw `events` for now.

## Consequences

Hourly-from-minute chaining is available later; `EXPIRE AFTER` interactions need testing first.
