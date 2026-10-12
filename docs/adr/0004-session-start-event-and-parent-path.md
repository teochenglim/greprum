# ADR 0004 — Session start event and parent_path

Status: Accepted

## Context

Sessions and journeys must be additive counts that Flow can roll up.

## Decision

Emit `$session_start` and carry `parent_path` on `$pageview`.

## Consequences

The tracker must read UTM parameters before stripping the query string.
