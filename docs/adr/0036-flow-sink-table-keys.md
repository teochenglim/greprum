# ADR 0036 — Flow sink table keys

Status: Accepted
Spike: spike-item-07

## Context

Spike item 7: do flows grouped by `path`, `parent_path`, `is_new_visitor` create sink tables with sensible keys?

## Decision

Yes. Sink tables get the group columns as `PRIMARY KEY` (tags), `time_window` as time index and an extra `update_at` column. Never `SELECT *` from them.

## Consequences

Query builders select columns by name. Observed on `click_stats_1h`: `PRIMARY KEY (site_id, path)`.
