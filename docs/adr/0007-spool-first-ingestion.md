# ADR 0007 — Spool first ingestion

Status: Accepted

## Context

Acknowledged events must survive a crash or a database outage.

## Decision

Append to a local spool with group commit, then `204`; a drainer writes to GreptimeDB.

## Consequences

Needs a persistent volume. Up to the fsync interval can be lost on OS crash unless `strict_fsync`.
