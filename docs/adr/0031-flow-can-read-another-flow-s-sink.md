# ADR 0031 — Flow can read another flow's sink

Status: Accepted
Spike: spike-item-03

## Context

Spike item 3: can hourly rollups chain from minute rollups?

## Decision

Yes on v1.2.1: a flow created `FROM` another flow's sink table produced results. ADR 0022 still builds all rollups from raw `events` until `EXPIRE AFTER` and late-arrival behaviour of chains are tested.

## Consequences

Cheaper hourly rollups are possible later without a schema change.
