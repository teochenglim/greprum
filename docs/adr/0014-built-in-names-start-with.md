# ADR 0014 — Built-in names start with $

Status: Accepted

## Context

Custom and built-in names must stay apart.

## Decision

Built-ins start with `$`; the gateway drops unknown `$` names and counts them.

## Consequences

Custom names are lowercase `[a-z0-9_.:-]{1,64}`.
