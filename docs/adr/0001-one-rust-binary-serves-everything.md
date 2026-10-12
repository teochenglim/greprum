# ADR 0001 — One Rust binary serves everything

Status: Accepted

## Context

Self-hosting simplicity is the point of replacing PostHog plus ClickHouse, Kafka and Postgres.

## Decision

One Rust binary (axum) serves tracker, ingest, query API and UI. GreptimeDB is the only dependency.

## Consequences

Operators run two processes. Ingest and query share a process, so the spool needs a persistent volume.
