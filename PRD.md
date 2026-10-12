# PRD: GrepRUM

Open source product analytics and RUM in one Rust binary, on GreptimeDB. Version 6, 12 October 2026. Written to be handed to Claude Code for implementation.

## 1. Purpose

GrepRUM replaces a PostHog plus ClickHouse stack for teams that want product owner metrics (clicks, journeys, funnels, retention, sessions) without running ClickHouse, Kafka and Postgres. One binary serves the tracker, ingests events, answers queries and hosts the admin UI. GreptimeDB is the only dependency.

**Users**

- Product owner: reads dashboards, no SQL.
- Site developer: adds one script tag, optionally signs a user token on the backend.
- Operator: runs the binary beside GreptimeDB.

**v1 scope**

- One script tag gives page views, session starts, SPA route changes, clicks, form submits, rage clicks and dead clicks. Optional scroll depth, JS errors and INP.
- Custom events (`data-track`, `track()`), signed `identify()`, `reset()`, `consent()`, session sampling, page include or exclude lists.
- Screens: Overview, Pages, Elements, Journeys, Funnels, Retention, Sessions, Session detail, Events, Browsers and devices, Usage.
- Privacy safe defaults: no IP stored, no input values, no URL query strings.

Decisions and their reasons are in section 12 (ADR log). Open technical unknowns are the spike checklist in section 10.

## 2. Architecture

```
Browser --t.js--> GrepRUM binary (axum 0.7+)
                   GET  /t.js        embedded tracker
                   POST /collect     ingest
                   /api/v1/*         query API
                   /admin/*          dashboard
                        |
                   spool (WAL) --> drainer --Sink trait--> GreptimeDB (MySQL :4002)
                                                              events (append_mode)
                                                              Flow rollups
                   query API <------ sqlx read pool ----------+
```

- Write path: `/collect` then gate, validate and enrich, spool append, `204`. A drainer writes to GreptimeDB afterwards.
- Read path: `/api/v1/*` then `sqlx` with bound parameters, then a rollup or `events`, then JSON.
- `Sink` trait wraps GreptimeDB writes. v1 is `SqlxSink` (MySQL protocol). A native ingester can replace it later without touching the gateway.
- Site settings live in a config file (allowed origins, HMAC secret, sample rate, page lists, features). No settings screen in v1.
- GreptimeDB ports: HTTP 4000, gRPC 4001, MySQL 4002, PostgreSQL 4003. Binds to 127.0.0.1 by default. Pin one GreptimeDB version.

**Repo layout (suggested)**

```
tracker/        JS source, build to t.js (size budget enforced in CI)
src/main.rs
src/config.rs
src/collect/    gate, validate, enrich, priority
src/spool/      wal, group commit, drainer, checkpoint
src/sink/       trait, sqlx impl
src/query/      endpoints, SQL builders (fixed table match, bound params)
src/jobs/       reconcile, daily first-seen, sessions table (if needed)
src/migrate/    DDL and flows, `greprum migrate`
ui/             Grafana dashboards JSON first, embedded UI only if needed
docs/adr/       one file per ADR
```

## 3. Tracker

Starts from Umami's MIT tracker (keep the licence notice). Core (page views, session start, SPA routes, clicks, `track()`, batching) under 3 KB gzipped. Optional features load only if listed in `data-features`. Total under 5 KB gzipped.

```html
<script async src="https://t.example.com/t.js" data-site="pk_xxx"
        data-features="scroll,errors,vitals,pageleave"
        data-sample-rate="0.25" data-env="prod" data-release="1.4.2"></script>
```

### Events

| Event | Behaviour | Default |
| --- | --- | --- |
| `$session_start` | First event of a session. Carries entry path, referrer host, `is_new_visitor`, and `utm_source`, `utm_medium`, `utm_campaign` (read before the query string is stripped). | On |
| `$pageview` | On load and on `pushState`, `replaceState`, `popstate`. Query and hash removed. Carries `parent_path` (previous page in the session). `page(id, group)` sets a custom id and group. `data-auto-pageview="false"` disables automatic views. | On |
| `$click` | Only if the target or an ancestor matches the click allowlist. | On |
| custom | `data-track="name"` or `track(name, props)`. Names must not start with `$`. | On |
| `$identify` | `identify(signedToken)`. Sends the current `visitor_id`. | On |
| `$form_submit` | Form id, action, field names. Never values. | On |
| `$rage_click` | See rage click rules. | On |
| `$dead_click` | See dead click rules. | On |
| `$scroll` | 25, 50, 75, 100 percent, once per page view. | `scroll` |
| `$pageleave` | On `visibilitychange` or page change, with `duration_ms`. | `pageleave` |
| `$error` | `onerror`, `unhandledrejection`. Message and stack, truncated. | `errors` |
| `$web_vital` | INP via `PerformanceObserver`. | `vitals` |

API: `track`, `page`, `identify`, `reset` (rotates `visitor_id` and `session_id`), `setContext` (max 10 keys, added to later events), `consent("granted"|"denied")` (with `data-consent="required"` nothing is stored or sent until granted; `denied` clears ids).

### Click capture

Walk from the target up the DOM. Record the click if the target or an ancestor matches: `a, button, form, input, select, textarea, label, [role=button], [role=link], [role=tab], [role=menuitem], [data-track], [data-rum-click]`. `data-capture="all"` records every click. Record tag, `id`, `role`, text (max 50 chars, none inside `[data-private]`), `data-*` attributes and a short CSS path. Never record input, textarea or password values.

### Rage click

One `$rage_click` per burst of `click_count` (3) clicks within `threshold_px` (30) inside `timeout_ms` (1000). Ignore: inside `[data-rum-no-rage]`; text or `aria-label` containing `next`, `previous`, `prev`, or equal to `>`, `<`, `+`, `-`; text editing surfaces (textarea, text input, contenteditable). The text list is configurable.

### Dead click

One `$dead_click` when a click is followed by no DOM mutation, scroll, text selection or URL change within 1 second. Clicks on or inside a link are never dead. Skip `[data-rum-no-dead]`.

### Exclusions

- `[data-rum-no-capture]`: no capture for the element and its children.
- `[data-private]`: event kept, text masked as `***`.
- `data-include-pages` or `data-exclude-pages` (path patterns, never both).
- Site config can list attribute names to drop.

### Identity, session, sampling

- `visitor_id` in `localStorage`. `session_id` in `sessionStorage` with last activity time. New session after 30 minutes idle. The tracker owns session ids.
- `data-storage="cookie"` with `data-cookie-domain=".example.com"` stores both ids in first-party cookies on the top-level domain (for `www` to `app` journeys). Off by default.
- Sampling is per session: one draw per session against `data-sample-rate`. An unsampled session sends nothing. Every event carries its `sample_rate`.

### Delivery

- Batch and send with `fetch` and `keepalive`, or `sendBeacon` on `visibilitychange`. Content type `text/plain` (no CORS preflight).
- Every event gets the next `seq` in its session (stored in `sessionStorage`). Every batch carries `sent_at` (browser clock at send). Failed sends retry with the same `seq` values.
- Site key goes in the URL: `POST /collect?k=pk_xxx`, so the gateway can reject before reading the body.
- `/t.js` and `/collect` are served from the customer's domain, with configurable paths.

### Payload

```json
{
  "token": "<signed identity token, optional>",
  "visitor_id": "v_8f2c", "session_id": "s_91ab",
  "sent_at": 1760236806000, "sample_rate": 0.25,
  "env": "prod", "release": "1.4.2", "ctx": {"plan": "pro"},
  "events": [
    {"ts": 1760236800000, "seq": 0, "name": "$session_start", "path": "/pricing",
     "parent_path": "", "page_group": "pricing", "is_new_visitor": true,
     "referrer": "https://example.com/", "props": {"utm_source": "newsletter"}},
    {"ts": 1760236805000, "seq": 1, "name": "$click", "path": "/pricing",
     "page_group": "pricing",
     "element": {"tag": "button", "id": "cta", "text": "Start free",
                 "css": "main>div.hero>button#cta", "attrs": {}}}
  ]
}
```

### Names and caps

- Built-in names: `$session_start`, `$pageview`, `$pageleave`, `$click`, `$identify`, `$form_submit`, `$rage_click`, `$dead_click`, `$scroll`, `$web_vital`, `$error`. The gateway drops other `$` names and counts them.
- Custom names match `^[a-z0-9_.:-]{1,64}$`, lowercased by the tracker.

| Field | Cap |
| --- | --- |
| `visitor_id`, `session_id`, `user_id` | 200 chars. Longer rejects the event (no truncation). |
| `props` | 4 KB JSON |
| `ctx` | 10 keys, key 32 chars, value 200 chars |
| `page_group`, `env`, `release` | 64 chars |
| element text | 50 chars |
| `sample_rate` | greater than 0, up to 1, else stored as 1 |
| `seq` | non-negative integer, else reject event |
| batch | 50 events |
| body | 64 KB, enforced before buffering |

## 4. Identity and security

| Concern | Requirement |
| --- | --- |
| Ingest auth | Public write-only site key `pk_xxx`, checked against the site's Origin allowlist, rate limited per key. |
| Anonymous visitor | `visitor_id` from the tracker. Never derived from IP. |
| Logged-in user | `identify(token)`. JWT signed with a per-site HMAC secret, with `sub` (opaque id, not an email) and short `exp`. Gateway pins the algorithm, checks signature and expiry, stores `sub` as `user_id`, sets `is_identified`. Invalid token: event stored as anonymous. |
| Logout | `reset()`. |
| Admin API | Separate credentials. Static API keys stored hashed in config. Every query is scoped to the caller's sites. |
| Database | Separate GreptimeDB users for write and read if the pinned version supports it. |

Funnels and journeys key on `visitor_id`. `user_id` counts identified users. Forged anonymous events are possible with `curl`, so anonymous counts are indicative. Signed identity protects per-user data.

## 5. Data model

Test every statement on the pinned GreptimeDB version.

### events

```sql
CREATE TABLE IF NOT EXISTS events (
  ts             TIMESTAMP(3) NOT NULL,
  site_id        STRING,
  event_name     STRING,
  visitor_id     STRING SKIPPING INDEX,
  session_id     STRING SKIPPING INDEX,
  user_id        STRING SKIPPING INDEX,
  is_identified  BOOLEAN,
  is_new_visitor BOOLEAN,   -- true only on $session_start
  seq            INT,       -- per session, starts at 0
  is_late        BOOLEAN,   -- older than the shortest flow window on arrival
  path           STRING,
  page_group     STRING,
  parent_path    STRING,
  referrer_host  STRING,
  el_tag STRING, el_id STRING, el_text STRING, el_css STRING,
  el_attrs       STRING,    -- JSON text
  props          STRING,    -- JSON text
  ctx            STRING,    -- JSON text
  env STRING, release STRING,
  sample_rate    DOUBLE,
  device STRING, browser STRING, os STRING, country STRING,
  TIME INDEX (ts),
  PRIMARY KEY (site_id, event_name)
) WITH (append_mode = 'true');
```

- Only `site_id` and `event_name` are tags. Ids stay out of the primary key.
- `(session_id, seq)` identifies an event: ordering, duplicate detection, loss estimate. Funnels order by `ts` then `seq`.
- Paths are normalised at the gateway (no query, no hash, optional route templates).
- Table `ttl` sets retention.

### Rollups (Flow, built from raw `events`)

`EXPIRE AFTER` limits recomputation and does not delete data. Set it above the beacon late-arrival delay.

```sql
CREATE FLOW click_rollup_1h SINK TO click_stats_1h
EXPIRE AFTER '7 days'::INTERVAL AS
SELECT site_id, path, count(*) AS click_count,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$click'
GROUP BY site_id, path, time_window;

CREATE FLOW session_start_1h SINK TO session_starts_1h
EXPIRE AFTER '7 days'::INTERVAL AS
SELECT site_id, path, is_new_visitor, count(*) AS sessions,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$session_start'
GROUP BY site_id, path, is_new_visitor, time_window;

CREATE FLOW page_edges_1h SINK TO page_edges_1h_tbl
EXPIRE AFTER '7 days'::INTERVAL AS
SELECT site_id, parent_path, path, count(*) AS views,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE event_name = '$pageview'
GROUP BY site_id, parent_path, path, time_window;

-- unique counts: Flow has no count(DISTINCT), so store distinct rows, count at query time
CREATE FLOW visitor_hours_flow SINK TO visitor_hours
EXPIRE AFTER '7 days'::INTERVAL AS
SELECT DISTINCT site_id, visitor_id,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events GROUP BY site_id, visitor_id, time_window;

CREATE FLOW user_hours_flow SINK TO user_hours
EXPIRE AFTER '7 days'::INTERVAL AS
SELECT DISTINCT site_id, user_id,
       date_bin('1 hour'::INTERVAL, ts) AS time_window
FROM events WHERE is_identified = true
GROUP BY site_id, user_id, time_window;
```

Also create a 1-minute click rollup for the time series. Confirm sink table columns (an extra `update_at` may exist) and sink keys when grouping by path columns.

### Sessions list

Query raw `events` grouped by `session_id` for a bounded range (default last 24 hours, newest 200). If it misses the latency gate, `jobs/` writes a `sessions` table when a session has been idle 30 minutes. Funnel and retention follow the same fallback: daily summary tables from a job.

## 6. Metric definitions

| Metric | Definition |
| --- | --- |
| Visitor | One `visitor_id` (a browser profile). Unique visitors = distinct `visitor_id`. |
| Identified user | Distinct `user_id` from valid tokens. |
| Session | Events sharing a `session_id`; new after 30 min idle. Count of sessions = count of `$session_start`. |
| New or returning | `is_new_visitor` on `$session_start`. |
| Entry page | `path` of `$session_start`. |
| Pages per session | `$pageview` count divided by `$session_start` count. |
| Session duration | `max(ts) - min(ts)` per session, bounded range only. |
| Page transition | A `page_edges_1h_tbl` row: `parent_path` to `path`. |
| Active visitors (DAU, WAU, MAU) | Distinct visitors with a `$pageview` or `$click` in the period. Day boundary in the site time zone (for example Asia/Singapore). |
| Funnel | Ordered events by one `visitor_id` within `window_seconds` (required, hard filter). Steps in order, not necessarily consecutive. |
| Retention (day N) | Of visitors first seen on day 0, share with any event on day N. First-seen dates come from a daily job. |
| Top element | `$click` count grouped by `el_css` and `path`. |
| Scaled count | Sampled count times 1 divided by `sample_rate`. API returns scaled and unscaled. |
| Browser side loss (estimate) | Per session, (max `seq` + 1) minus distinct `seq` received, summed, divided by expected total. Gateway-shed events are subtracted. End-of-session loss is invisible. |
| Duplicate rate | Events whose `(session_id, seq)` already exists, divided by all events. Computed by the reconcile job. |

## 7. Ingestion flow

Spool first: acknowledge only after the batch is on local disk.

```
Tracker (seq per event, sent_at per batch)
  POST /collect?k=pk_xxx
Gateway
  1 gate        key, Origin, rate limit, size (before reading body)
  2 validate    caps, names, URL normalise, GeoIP, drop IP, bot filter,
                is_identified, clock fix, dedup, priority class
  3 shed check  class against spool fill level
  4 spool       append frame, group commit, fsync every 100 ms or 1 MB, then 204
  5 drainer     fresh lane first, backfill lane second -> Sink::write
  6 checkpoint  write temp file, rename; delete finished segments
  7 reconcile   hourly: duplicates and seq gaps -> metrics
```

**Rules**

- Spool frame: length prefixed, checksummed, one frame per accepted batch, in 64 MB segment files.
- Group commit task writes frames from concurrent requests together. `204` returns once the write reaches the OS. Config `strict_fsync = true` acknowledges only after `fsync`.
- Clock fix: `ts = received_at - (sent_at - ts)`. More than 5 minutes ahead of the gateway: set to `received_at`. Older than 7 days: drop and count. Older than 1 day: keep, set `is_late = true`. No `sent_at`: no correction.
- Dedup: bounded in-memory cache of recent `(session_id, seq)` per site, about 10 minutes. A hit drops the event and counts it.
- Drainer: fresh lane inserts every 1 second or 500 events. Backfill lane replays older segments, up to 5,000 events per insert, at lower priority. Retry with exponential backoff and jitter. A frame the database rejects for good moves to `dead/`, is counted, and does not block the lane.
- Delivery is at-least-once. Duplicate rows after a lost insert reply are measured, not removed.

**Priority and shedding**

| Class | Events | Shed when spool is above |
| --- | --- | --- |
| P0 | `$session_start`, `$pageview`, `$click`, `$identify`, `$form_submit`, custom | Never. `503` only when the spool is full. |
| P1 | `$rage_click`, `$dead_click`, `$error` | 80 percent |
| P2 | `$scroll`, `$web_vital`, `$pageleave` | 50 percent |

Shed counts are kept by class, so the loss estimate does not mistake shedding for browser loss.

**Guarantees**

| Situation | Result |
| --- | --- |
| Gateway crash or restart | No acknowledged event lost. Replay from checkpoint. |
| OS crash or power loss | Up to the `fsync` interval can be lost. None in strict mode. |
| GreptimeDB down or slow | Spool grows to the disk cap, then shed by class, then `503`. Backfill drains after recovery. |
| Client retry | Dropped by dedup cache if it hits the same instance within the window. |
| Browser closes before sending | Lost, seen later as a `seq` gap. |
| Several gateways | Each has its own spool and cache. A retry landing elsewhere can duplicate. Measured. |

**Defaults**: segment 64 MB, `fsync` 100 ms or 1 MB, spool cap 2 GB, shed P2 50 percent and P1 80 percent, dedup 10 minutes, fresh batch 500 events or 1 s, backfill batch 5,000, future clamp 5 minutes, late 1 day, drop older than 7 days. These are starting values to tune in the spike.

**Metrics** (Prometheus): accepted, rejected, dropped by reason, shed by class, spool size and fill, age of oldest unwritten event, drain rate per lane, insert latency, duplicates dropped, late events, dead frames, estimated browser side loss, duplicate rate, events per site per day.

The binary needs a persistent volume for the spool.

## 8. Gateway and query API

### Ingest

- `GET /t.js` serves the embedded tracker with cache headers and a content hash in the URL.
- `POST /collect` reads the raw body and parses JSON itself (`sendBeacon` sends `text/plain`). Returns `204`. Add a CORS layer for `application/json` preflight.
- `DefaultBodyLimit` 64 KB. Reject batches over 50 events.
- Check key and Origin before parsing events. Verify token if present.
- `/healthz` and Prometheus metrics.

### Query API

| Endpoint | Purpose | Source |
| --- | --- | --- |
| `GET /api/v1/stats/overview` | DAU, WAU, MAU, identified users, sessions, clicks, page views | `visitor_hours`, `user_hours`, `session_starts_1h`, click rollups |
| `GET /api/v1/stats/timeseries` | Clicks or page views over time | click rollups |
| `GET /api/v1/stats/top-pages` | Pages and page groups by views, clicks, visitors | rollups, `events` |
| `GET /api/v1/stats/entry-pages` | Entry pages by sessions, new against returning | `session_starts_1h` |
| `GET /api/v1/stats/journeys` | Page transitions, optionally from one page | `page_edges_1h_tbl` |
| `GET /api/v1/stats/top-elements` | Click targets by `el_css` and path | `events` |
| `GET /api/v1/stats/funnel?window_seconds=N` | Conversion between events. Missing `window_seconds` returns `400`. | `events` |
| `GET /api/v1/stats/retention` | Cohort retention | daily first-seen table |
| `GET /api/v1/stats/rage-clicks` | Elements by rage and dead clicks against clicks | `events` |
| `GET /api/v1/stats/browsers-devices` | Visitors and sessions by browser, OS, device, country | `events`, rollups |
| `GET /api/v1/sessions` | Session list for a bounded range, with filters | `events` |
| `GET /api/v1/sessions/{session_id}` | Ordered events of one session | `events` |
| `GET /api/v1/events` | Raw events, paginated, filtered | `events` |
| `GET /api/v1/usage` | Events per site per day: accepted, rejected, dropped | gateway metrics |

### SQL safety

- Table names come from a fixed `match`, never from request text.
- Every value is a bound parameter. No `format!` on values.
- Every query includes `site_id = ?` from the authorised site list.
- Time ranges are capped per endpoint (for example 31 days on raw `events`, 24 hours default on `/sessions`).

## 9. Screens

Ship Grafana dashboards over the query API or MySQL protocol first. Build the embedded UI only if Grafana cannot cover funnel step building, session drill-down or a journey diagram.

| Screen | Question it answers | Source |
| --- | --- | --- |
| Overview | Visitors, sessions, page views, clicks, this week against last | rollups, hour tables |
| Pages | Which pages get views, visitors, clicks? | rollups, `events` |
| Elements | What do people click, and where do they get frustrated? | `events` |
| Journeys | Where do people go next from a page? | `page_edges_1h_tbl`, `session_starts_1h` |
| Funnels | Where do users drop off? | `events` |
| Retention | Who comes back? | first-seen table |
| Sessions | Which sessions had rage clicks or errors? Filter by user, event, country, device. | `events` |
| Session detail | What did one visitor do, in order? | `events` |
| Events | What do raw events look like? | `events` |
| Browsers and devices | Which browsers, systems, devices, countries? | `events`, rollups |
| Usage | Events per site per day, accepted, rejected, dropped | gateway metrics |

Grafana limits: funnel steps chosen by dashboard variables; session detail opens through a table link passing `session_id`; journeys show as a transitions table.

## 10. Work plan and spike checklist

Phase 0 gates everything else. Each phase has an acceptance test that Claude Code must write and pass.

| Phase | Deliverable | Acceptance |
| --- | --- | --- |
| 0. Spike | Synthetic generator for 10 to 50 million events. Test `sqlx` multi-row inserts, spool append and drain with a forced crash, skipping against inverted index on ids, the five flows, a funnel query on `visitor_id`, a retention query, the Sessions query over 24 hours, `count(DISTINCT)` on raw rows. | Rollup queries p95 under 2 s. Funnel, retention, Sessions under 10 s on 50 million events, or the summary-table jobs are adopted. |
| 1. Tracker | Section 3 in full. | Core under 3 KB gz, total under 5 KB. Under 50 ms main thread blocking on a demo site. Batches flush on tab close. Unsampled session sends nothing. |
| 2. Schema | `greprum migrate` creates tables and flows. | Runs clean on the pinned GreptimeDB version. |
| 3. Gateway | `/t.js`, `/collect`, spool, drainer, `Sink`, checks, caps, enrichment, metrics. | Load test with no silent loss: accepted equals written plus counted drops, including after a forced crash and a 10 minute GreptimeDB outage. |
| 4. Query API | Section 8 endpoints with auth and site scoping. | One site's key cannot read another site. No SQL built from request text. `window_seconds` required on funnel. |
| 5. Grafana | Dashboards for section 9. | A product owner answers "which page gets the most clicks?" and "where do users drop off in the signup funnel?" without SQL. |
| 6. Embedded UI | Only if phase 5 shows a gap. | Decided after phase 5 feedback. |

**Success metrics**

- 99.9 percent of accepted events reach GreptimeDB within 5 seconds, measured as the age of the oldest unwritten event in the spool.
- Tracker stays within its size and main thread budgets.

**Spike checklist (answer in `docs/adr/` as each is resolved)**

1. Can rows be deleted from an append-only table, for erasure requests?
2. Does `sqlx` work with GreptimeDB's MySQL protocol for multi-row inserts and prepared statements?
3. Can a flow read another flow's sink, so hourly rollups chain from minute rollups? (Until confirmed, build all rollups from raw `events`.)
4. Delay from insert to a visible flow result. Does `ADMIN FLUSH_FLOW` help tests?
5. `count(DISTINCT)` latency on 50 million raw rows.
6. Separate database users with different permissions.
7. Do flows grouped by `path`, `parent_path`, `is_new_visitor` create sink tables with sensible keys?
8. Can a timed-out insert be detected as applied? Any idempotency over the MySQL protocol?
9. Is a 100 ms `fsync` interval acceptable, and is a 10 minute dedup cache enough?
10. Should noisy events (scroll, vitals) go to a second table with a shorter TTL?
11. Can Grafana give a usable funnel and session drill-down?

## 11. Privacy and retention

Defaults store no IP, no input values and no query strings. Whoever owns PDPA compliance for the deploying organisation must review this before launch.

- IP is used for GeoIP country and a short-lived rate limit hash, then discarded.
- Inputs and `[data-private]` content are masked in the browser before sending. `[data-rum-no-capture]` stops capture.
- Element text cut to 50 chars, and can be turned off.
- Query strings and hashes removed, except the three `utm_` parameters on session start.
- `user_id` is an opaque id. Screens show this id only. No emails stored.
- `ctx` values are chosen by the site and must not hold personal data.
- Honour Do Not Track when the site setting is on.
- Retention by table `ttl`. Default 13 months, to be confirmed.
- Erasure by user depends on spike item 1. Until then rely on opaque ids and retention.

## 12. ADR log

One file per decision in `docs/adr/NNNN-title.md` (status, context, decision, consequences). Claude Code adds new ADRs when a spike result or implementation choice changes anything below. Statuses: Accepted, Superseded, Open.

| ADR | Decision | Reason | Status |
| --- | --- | --- | --- |
| 0001 | One Rust binary (axum) serves tracker, ingest, query API and UI. GreptimeDB is the only dependency. | Self-hosting simplicity, the point of replacing PostHog plus ClickHouse. | Accepted |
| 0002 | Funnels, journeys and unique counts key on `visitor_id`. `user_id` counts identified users. No combined actor id. | A combined id splits one person into two at login and breaks signup funnels. | Accepted |
| 0003 | Tracker owns `session_id` (30 min idle). Gateway keeps no session state. | Gateway stays stateless apart from spool and caches. | Accepted |
| 0004 | `$session_start` event and `parent_path` on `$pageview`. | Sessions and journeys become additive counts that Flow can roll up. | Accepted |
| 0005 | Unique counts use `SELECT DISTINCT` hour tables, counted at query time. | Flow has no `count(DISTINCT)`. | Accepted, verify in spike |
| 0006 | Ids use `SKIPPING INDEX`, not inverted. Only `site_id` and `event_name` are tags. | Inverted suits low cardinality, skipping suits ids. | Accepted, compare in spike |
| 0007 | Spool-first ingestion with group commit, then `204`. | Acknowledged events survive a crash or a database outage. | Accepted |
| 0008 | Per-session `seq` identifies events. Delivery is at-least-once. Duplicates are dropped by cache and measured. | The table is append-only and does not deduplicate. | Accepted |
| 0009 | Event time corrected with `sent_at`, clamped, ordered by `seq`. | Browser clocks are often wrong. | Accepted |
| 0010 | Overload sheds by priority class (P2, P1, then `503` for P0 only when full). | Core events survive longest. | Accepted |
| 0011 | Sampling is per session, with `sample_rate` stored on each event. | Sampled sessions stay complete, so funnels stay valid. Counts can be scaled. | Accepted |
| 0012 | Click capture uses an element allowlist walked from target to root, plus roles and opt-in attributes. | Avoids noise but keeps clickable `div`s. | Accepted |
| 0013 | Rage click uses 3 clicks, 30 px, 1000 ms with ignore rules. Dead click uses no DOM change, scroll, selection or URL change in 1 s, and links never count. | Defaults match PostHog's documented behaviour and limit false positives. | Accepted |
| 0014 | Built-in event names start with `$`. Gateway drops unknown `$` names. | Keeps custom and built-in names apart. | Accepted |
| 0015 | Over-long ids reject the event. No truncation. | Truncation merges different ids. | Accepted |
| 0016 | Element data stored in flat columns. | Top elements are plain SQL. | Accepted |
| 0017 | Funnel `window_seconds` is required. | Prevents unbounded and misleading funnels. | Accepted |
| 0018 | `Sink` trait with `SqlxSink` first. | Allows a native ingester later without gateway changes. | Accepted |
| 0019 | Site key in the URL, Origin allowlist, signed JWT for identity. | `sendBeacon` cannot set headers. Public key proves site only. | Accepted |
| 0020 | URLs lose query and hash. Only three `utm_` parameters read on session start. | Query strings often hold tokens and personal data. | Accepted |
| 0021 | Web vitals: INP only. | Other metrics are deprecated or out of scope. | Accepted |
| 0022 | All rollups built from raw `events`. | Chaining flows is not confirmed. | Open (spike 3) |
| 0023 | Grafana first, embedded UI only if needed. | Fastest route to product owner screens. | Accepted |
| 0024 | Query API uses fixed table names, bound parameters and mandatory `site_id`. | Prevents SQL injection and cross-site reads. | Accepted |
| 0025 | Tracker and gateway served from the customer's domain, paths configurable. | Ad blockers target third-party analytics domains. | Accepted |
| 0026 | Optional first-party cookie on the top-level domain. | Needed for subdomain journeys. Off by default for consent reasons. | Accepted |
| 0027 | Licence for binary and tracker (MIT or Apache 2.0 against AGPL-3). Project name GrepRUM. | Decide before first public release. | Open |

Licence notes: Umami (MIT) tracker code is reused with its notice kept. Do not copy code from AGPL-3 projects.