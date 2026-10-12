# ADR 0018 — Sink trait with SqlxSink first

Status: Accepted

## Context

A native ingester may replace MySQL protocol later.

## Decision

`Sink` trait; `SqlxSink` first.

## Consequences

The gateway never names sqlx outside `sink/`.
