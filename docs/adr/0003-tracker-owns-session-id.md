# ADR 0003 — Tracker owns session_id

Status: Accepted

## Context

The gateway should stay stateless apart from spool and caches.

## Decision

The tracker owns `session_id` (30 minutes idle). The gateway keeps no session state.

## Consequences

Forged or clock-skewed sessions are possible; session counts are indicative.
