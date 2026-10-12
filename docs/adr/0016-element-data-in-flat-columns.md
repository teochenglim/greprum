# ADR 0016 — Element data in flat columns

Status: Accepted

## Context

Top elements should be plain SQL.

## Decision

Store `el_tag`, `el_id`, `el_text`, `el_css` as columns; attrs as JSON text.

## Consequences

Wide table; empty optionals are stored as NULL.
