# ADR 0040 — Grafana funnel and drill-down

Status: Accepted
Spike: spike-item-11

## Context

Spike item 11: can Grafana give a usable funnel and session drill-down?

## Decision

Grafana covers the funnel (step variables + window), the session drill-down (table data link plus a newest-session default) and journeys as a transitions table. Verified by rendering all 11 dashboards in Grafana 11.6 on seeded data (`images/grafana_*.png`) and by clicking through from Sessions to Session detail.

## Consequences

The journey diagram remains the only gap (ADR 0044). Bugs found only by looking at the rendered output (and fixed): NULL columns hid every session, retention showed raw day indexes, the funnel chart plotted the step number, Prometheus panels used the wrong target field.
