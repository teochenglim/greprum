# ADR 0006 — Skipping index on ids

Status: Accepted

## Context

Inverted suits low cardinality, skipping suits ids.

## Decision

Ids use `SKIPPING INDEX`. Only `site_id` and `event_name` are tags.

## Consequences

Compare against an inverted index in the spike if id lookups prove slow.
