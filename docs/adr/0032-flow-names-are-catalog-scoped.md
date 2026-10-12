# ADR 0032 — Flow names are catalog-scoped

Status: Accepted

## Context

A second database in the same catalog silently skipped `CREATE FLOW IF NOT EXISTS` because a flow with that name existed in another database; its sink tables were never created.

## Decision

Add `greptime.flow_prefix` (default empty). Tests use a unique prefix per database.

## Consequences

Operators sharing a catalog between installs must set distinct prefixes.
