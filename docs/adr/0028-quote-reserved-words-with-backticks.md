# ADR 0028 — Quote reserved words with backticks

Status: Accepted

## Context

`path` and `release` are reserved words in GreptimeDB's SQL parser. Double quotes work over the HTTP SQL API but are string literals over the MySQL protocol, which is what sqlx uses.

## Decision

Quote with backticks everywhere (DDL, flows, inserts, queries).

## Consequences

A grep-able rule in AGENTS.md. Spike run on v1.2.1.
