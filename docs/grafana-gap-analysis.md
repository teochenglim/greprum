# Grafana gap analysis (input to the v1.0.0 embedded-UI decision)

PRD §9: ship Grafana first; build the embedded UI only if Grafana cannot cover funnel step
building, session drill-down or a journey diagram. Dashboards: `ui/grafana/dashboards/`.

## Funnel steps

Evidence: `greprum-funnels` — steps chosen with `step1..step4` text variables and
`window_seconds` custom variable; the SQL is the same ordered-step query as the API and was
executed on GreptimeDB v1.2.1 by `ac_0_6_2_d`.

Verdict: **Covered**, with limits: exactly four steps, and step names are typed (no
autocomplete of event names beyond a query variable).

## Session drill-down

Evidence: `greprum-sessions` table carries a data link to `greprum-session-detail`
passing `session_id`, `site` and the time range.

Verdict: **Covered**. Opening a session is one click; the detail table is ordered by `ts, seq`.

## Journey diagram

Evidence: `greprum-journeys` shows page transitions as a table (`from_page`, `to_page`,
`views`), filterable by `from_page`. Core Grafana has no Sankey/flow panel.

Verdict: **Gap (diagram only)**. The transitions table answers "where do people go next?".
A diagram needs a community panel plugin or the embedded UI. Not a blocker for v1.

## Recommendation

No embedded UI for v1.0.0. Revisit if product owners ask for a visual journey diagram or
a funnel builder with more than four steps.
