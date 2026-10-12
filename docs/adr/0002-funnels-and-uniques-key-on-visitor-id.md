# ADR 0002 — Funnels and uniques key on visitor_id

Status: Accepted

## Context

A combined actor id splits one person into two at login and breaks signup funnels.

## Decision

Funnels, journeys and unique counts key on `visitor_id`. `user_id` counts identified users. There is no combined actor id.

## Consequences

Cross-device identity is out of scope; identified-user counts are reported separately.
