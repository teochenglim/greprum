# ADR 0024 — Query API safety

Status: Accepted

## Context

Prevent SQL injection and cross-site reads.

## Decision

Fixed table names, bound parameters, mandatory `site_id = ?`.

## Consequences

Enforced by builder tests and fuzzing.
