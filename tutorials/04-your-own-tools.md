# 4. Use your own tools on GreptimeDB

GrepRUM's data is ordinary tables in GreptimeDB. Anything that speaks the MySQL protocol, the PostgreSQL
protocol or GreptimeDB's HTTP SQL endpoint can read it: `mysql`, DBeaver, Metabase, Superset, a notebook,
or the Grafana you already run.

| Protocol | Address (compose) |
|---|---|
| MySQL | `127.0.0.1:4002`, user `root`, no password, database `public` |
| PostgreSQL | `127.0.0.1:4003` |
| HTTP SQL | `http://127.0.0.1:4000/v1/sql` |

## Look around

```bash
mysql -h 127.0.0.1 -P 4002 -u root public -e "SHOW TABLES"
mysql -h 127.0.0.1 -P 4002 -u root public -e \
  "SELECT event_name, count(*) n FROM events WHERE site_id = 'demo' GROUP BY event_name ORDER BY n DESC"
```

No MySQL client? Use HTTP:

```bash
curl -s http://127.0.0.1:4000/v1/sql --data-urlencode \
  "sql=SELECT event_name, count(*) FROM events WHERE site_id='demo' GROUP BY event_name"
```

> `path` and `release` are reserved words in GreptimeDB: quote them with backticks.
> Always filter on `site_id`.

## Tables

| Table | Written by | Holds |
|---|---|---|
| `events` | gateway drainer | one row per event, append-only, TTL 13 months (configurable) |
| `click_stats_1h`, `click_stats_1m` | Flow | clicks per page per hour / per site per minute |
| `session_starts_1h` | Flow | sessions per entry page and new/returning |
| `page_edges_1h_tbl` | Flow | page-to-page transitions |
| `visitor_hours`, `user_hours` | Flow | distinct visitor / user per hour (DAU/WAU/MAU, retention) |
| `first_seen` | scheduled job | first hour each visitor appeared |
| `sessions` | scheduled job | one row per session |

Rollup tables carry an extra `update_at` column from GreptimeDB: select named columns, not `*`.

### `events` columns

| Column | Meaning |
|---|---|
| `ts` | event time (ms, time index) |
| `site_id` | site, from the key |
| `event_name` | `$pageview`, `$click`, `$session_start`, `$rage_click`, `$error`, your own names |
| `visitor_id`, `session_id`, `user_id` | opaque ids; `user_id` only after `identify()` |
| `is_identified`, `is_new_visitor` | flags |
| `seq` | per-session sequence number |
| `is_late` | arrived after its session ended |
| `path`, `page_group`, `parent_path`, `referrer_host` | page, grouping, previous page, referrer host |
| `el_tag`, `el_id`, `el_text`, `el_css`, `el_attrs` | clicked element |
| `props`, `ctx` | event properties and context (JSON) |
| `env`, `release` | from `data-env`, `data-release` |
| `sample_rate` | multiply counts by `1 / sample_rate` for estimates |
| `device`, `browser`, `os`, `country` | from the user agent and a trusted country header |

## Example queries

```sql
-- top pages by clicks, last 7 days
SELECT `path`, count(*) AS clicks
FROM events
WHERE site_id = 'demo' AND event_name = '$click' AND ts > now() - INTERVAL '7 days'
GROUP BY `path` ORDER BY clicks DESC LIMIT 10;

-- sessions with rage clicks
SELECT session_id, count(*) AS rage
FROM events
WHERE site_id = 'demo' AND event_name = '$rage_click'
GROUP BY session_id ORDER BY rage DESC LIMIT 10;
```

## Point your own Grafana at it

Add a **MySQL** data source: host `<greptime>:4002`, database `public`, user `root` (use a read-only user in
production, see [tutorial 5](05-going-to-production.md)). Then import the dashboards from
[ui/grafana/dashboards/](../ui/grafana/dashboards/): *Dashboards → New → Import → upload JSON*. They expect
a data source with uid `greprum-greptime`; provisioning one is
[ui/grafana/provisioning/datasources/greprum.yml](../ui/grafana/provisioning/datasources/greprum.yml).

## Or use the HTTP query API

The API adds what SQL does not give you for free: per-site API keys, time caps, funnels and retention in one
call, scaled and unscaled counts.

```bash
curl -s -H "Authorization: Bearer demo-api-key" \
  "http://127.0.0.1:8080/api/v1/stats/top-pages?site=demo"
curl -s -H "Authorization: Bearer demo-api-key" \
  "http://127.0.0.1:8080/api/v1/stats/funnel?site=demo&steps=%24pageview,signup&window_seconds=1800"
```

Endpoint list: [README.md](../README.md#query-api).

Next: [going to production](05-going-to-production.md).
