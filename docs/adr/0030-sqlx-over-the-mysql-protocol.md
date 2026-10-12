# ADR 0030 — sqlx over the MySQL protocol

Status: Accepted
Spike: spike-item-02

## Context

Spike item 2: does sqlx work with GreptimeDB's MySQL protocol for multi-row inserts and prepared statements?

## Decision

Yes. Multi-row `INSERT` with bound parameters works (1 000 rows in one statement, hostile strings round-trip). DDL must use the text protocol (`raw_sql`).

## Consequences

`SqlxSink` chunks at 65 535 placeholders / 28 columns. Verified by `ac_0_4_16_*` live tests.
