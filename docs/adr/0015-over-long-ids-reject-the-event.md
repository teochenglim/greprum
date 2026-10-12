# ADR 0015 — Over-long ids reject the event

Status: Accepted

## Context

Truncation merges different ids.

## Decision

Ids over 200 characters reject; no truncation.

## Consequences

A buggy integration shows up as rejects, not silent merging.
