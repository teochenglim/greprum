# ADR 0011 — Sampling per session

Status: Accepted

## Context

Sampled sessions stay complete, so funnels stay valid.

## Decision

One draw per session; `sample_rate` stored on each event; API returns scaled and unscaled.

## Consequences

Rollups carry `sum(1/sample_rate)` columns.
