# ADR 0038 — fsync interval and dedup window

Status: Accepted
Spike: spike-item-09

## Context

Spike item 9: is a 100 ms fsync interval acceptable and is a 10 minute dedup cache enough?

## Decision

Defaults kept: fsync every 100 ms or 1 MB, dedup window 10 minutes. Evidence: 20 real `kill -9` rounds lose nothing (ADR 0048); sustained 3 000 events/s keeps the oldest unwritten age at 1.1 s (ADR 0050); a 10 minute outage loses nothing (ADR 0049).

## Consequences

`strict_fsync = true` remains the choice for operators who cannot accept losing up to 100 ms on an OS crash or power loss; it was not exercised under a power-cut test.
