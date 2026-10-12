# DESIGN 00 — Requirements Traceability

Every requirement in `PRD.md` has an ID here. Every ID must appear in the `Reqs` column of **at least one AC** in
`RELEASE/vX.Y.Z.md`, and every AC must be governed by **at least two tests** (`scripts/ac-map.sh`, run by
`make ac-map` and `make check`). Chain: **PRD § → R-ID → AC → tests**.

Format is parsed by the script: keep `| R-XX | … |` rows and the `AC` ids in release files intact.

| ID | PRD § | Requirement |
|---|---|---|
| R-T1 | 3 Events | `$session_start` is the first event of a session with entry path, referrer host, `is_new_visitor`, `utm_*` read before query strip |
| R-T2 | 3 Events | `$pageview` on load + `pushState`/`replaceState`/`popstate`; query+hash removed; `parent_path`; `page(id, group)`; `data-auto-pageview="false"` |
| R-T3 | 3 Click capture | `$click` only when target/ancestor matches allowlist; `data-capture="all"`; records tag, id, role, text ≤50, data-* attrs, css path; never input values |
| R-T4 | 3 Events | custom events via `data-track` / `track()`; names never start with `$`; lowercased |
| R-T5 | 3 API | `identify`, `reset` (rotates ids), `setContext` (≤10 keys), `consent` (required ⇒ nothing stored/sent until granted; denied clears ids) |
| R-T6 | 3 Events | `$form_submit` carries form id, action, field names, never values |
| R-T7 | 3 Rage click | one `$rage_click` per burst of 3 clicks / 30 px / 1000 ms with ignore rules (no-rage attr, next/prev text, text-edit surfaces); configurable |
| R-T8 | 3 Dead click | one `$dead_click` when no DOM mutation, scroll, selection or URL change within 1 s; links never dead; `data-rum-no-dead` |
| R-T9 | 3 Events | `$scroll`, `$pageleave`, `$error`, `$web_vital`(INP) load only if listed in `data-features` |
| R-T10 | 3 Exclusions | `data-rum-no-capture`, `data-private` masking `***`, include/exclude page lists (never both), site attr drop list |
| R-T11 | 3 Identity | `visitor_id` localStorage; `session_id` sessionStorage, new after 30 min idle; optional first-party cookie on top-level domain |
| R-T12 | 3 Sampling | one draw per session vs `data-sample-rate`; unsampled sends nothing; every event carries `sample_rate` |
| R-T13 | 3 Delivery | batch; `fetch keepalive`/`sendBeacon` on `visibilitychange`; `text/plain`; per-session `seq`; batch `sent_at`; retry with same `seq`; key in URL |
| R-T14 | 3, 10 | core ≤ 3 KB gz, total ≤ 5 KB gz; < 50 ms main-thread blocking; batches flush on tab close |
| R-T15 | 3 Payload | payload shape and client-side caps/names match the gateway contract |
| R-G1 | 8 Ingest | `GET /t.js` embedded tracker, cache headers, content hash in URL, configurable path |
| R-G2 | 8 Ingest | `POST /collect` parses raw body itself, returns 204, CORS layer for `application/json` preflight |
| R-G3 | 7 Gateway, 4 | gate before body read: site key, Origin allowlist, rate limit per key, size; 64 KB body limit |
| R-G4 | 3 Caps | field caps (ids 200 reject, props 4 KB, ctx 10/32/200, group/env/release 64, el text 50, batch 50) |
| R-G5 | 3 Names | built-in `$` names allowed, other `$` dropped+counted; custom `^[a-z0-9_.:-]{1,64}$` |
| R-G6 | 7 step 2 | enrichment: UA → device/browser/os, country, bot filter, URL normalise, `is_identified`; IP discarded |
| R-G7 | 4 Identity | JWT HS256 pinned, signature+expiry checked, `sub`→`user_id`; invalid ⇒ anonymous |
| R-G8 | 7 Rules | clock fix `ts = received_at-(sent_at-ts)`; >5 min future clamp; >1 d `is_late`; >7 d drop+count; no `sent_at` ⇒ none |
| R-G9 | 7 Rules | dedup cache `(session_id, seq)` per site ~10 min; hit drops+counts |
| R-G10 | 7 Priority | P0/P1/P2 classes; shed P2 at 50 %, P1 at 80 %; P0 `503` only when full; shed counts by class |
| R-G11 | 7 Rules | spool frames length-prefixed+checksummed, one per batch, 64 MB segments, group commit, fsync 100 ms/1 MB, `strict_fsync`, disk cap |
| R-G12 | 7 Rules | drainer fresh lane (500/1 s) + backfill lane (5000), backoff+jitter, permanent failures to `dead/`, checkpoint temp+rename, delete finished segments |
| R-G13 | 7 Guarantees | crash/restart loses no acknowledged event; DB outage grows spool then sheds then 503; at-least-once |
| R-G14 | 7 Metrics, 8 | `/healthz` + Prometheus metrics listed in PRD §7 |
| R-G15 | 7, 6 | reconcile job: duplicates and `seq` gaps → loss estimate and duplicate rate |
| R-G16 | 3 Caps | `sample_rate` >0 and ≤1 else stored as 1; `seq` non-negative int else reject |
| R-G17 | 2, ADR 0018 | `Sink` trait; `SqlxSink` multi-row insert with bound params over MySQL protocol |
| R-S1 | 5 events | `events` table DDL: columns, `TIME INDEX`, PK only `(site_id, event_name)`, skipping index on ids, `append_mode` |
| R-S2 | 5 Rollups | flows built from raw `events`: click, session_start, page_edges, visitor_hours, user_hours, 1-min click |
| R-S3 | 10 Phase 2 | `greprum migrate` creates tables+flows, idempotent, clean on pinned GreptimeDB |
| R-S4 | 5, 11 | retention through table `ttl` (default 13 months) |
| R-S5 | 5 Sessions | daily first-seen job and summary-table fallback for sessions/funnel/retention |
| R-Q1 | 8 | `GET /stats/overview` DAU/WAU/MAU, identified users, sessions, clicks, page views |
| R-Q2 | 8 | `GET /stats/timeseries` clicks or page views over time |
| R-Q3 | 8 | `GET /stats/top-pages` by views, clicks, visitors; path or page group |
| R-Q4 | 8 | `GET /stats/entry-pages` sessions, new vs returning |
| R-Q5 | 8 | `GET /stats/journeys` page transitions, optionally from one page |
| R-Q6 | 8 | `GET /stats/top-elements` clicks by `el_css` and path |
| R-Q7 | 8, 6 | `GET /stats/funnel` ordered events per `visitor_id` within required `window_seconds` (400 if missing) |
| R-Q8 | 8, 6 | `GET /stats/retention` day-N cohort retention from first-seen |
| R-Q9 | 8 | `GET /stats/rage-clicks` rage and dead clicks vs clicks per element |
| R-Q10 | 8 | `GET /stats/browsers-devices` visitors+sessions by browser/os/device/country |
| R-Q11 | 8 | `GET /sessions` bounded range (24 h default, newest 200) with filters |
| R-Q12 | 8 | `GET /sessions/{id}` ordered events of one session |
| R-Q13 | 8 | `GET /events` raw, paginated, filtered |
| R-Q14 | 8 | `GET /usage` events per site per day accepted/rejected/dropped |
| R-Q15 | 4 Admin API, 8 | static API keys stored hashed; every query scoped to the caller's sites |
| R-Q16 | 8 SQL safety | fixed table `match`, bound params only, mandatory `site_id = ?` |
| R-Q17 | 8 SQL safety | per-endpoint time-range caps (31 d raw events, 24 h default sessions) |
| R-Q18 | 6 | scaled (`× 1/sample_rate`) and unscaled counts both returned |
| R-Q19 | 6 | metric definitions: DAU/WAU/MAU in site time zone, pages/session, session duration, new vs returning, top element |
| R-D1 | 9 | Grafana dashboards for Overview, Pages, Elements, Journeys, Funnels, Retention, Sessions, Session detail, Events, Browsers & devices, Usage |
| R-D2 | 10 Phase 5 | product owner answers "most clicked page" and "signup funnel drop-off" without SQL |
| R-D3 | 9 | Grafana limits handled: funnel steps via variables, session drill-down via table link, journeys as transitions table |
| R-P1 | 11 | no IP stored; used only for GeoIP/rate-limit then discarded |
| R-P2 | 11 | input values and `[data-private]` masked in browser; element text ≤50 and can be disabled |
| R-P3 | 11 | query strings and hashes removed except three `utm_` on session start |
| R-P4 | 11 | `user_id` opaque; no emails stored; `ctx` never auto-filled |
| R-P5 | 11 | Do Not Track honoured when site setting on |
| R-P6 | 11 | retention default 13 months via `ttl` (confirmed with owner) |
| R-P7 | 11, spike 1 | erasure path documented against spike item 1 result |
| R-X1 | 10 Spike | 11 spike checklist items answered as ADRs in `docs/adr/` |
| R-X2 | 12 | ADR log: one file per decision `docs/adr/NNNN-title.md` with status, context, decision, consequences |
| R-X3 | 3, licence | tracker is original code (no third-party tracker source vendored); no AGPL-3 code copied |
| R-X4 | 2 | binds `127.0.0.1` by default; GreptimeDB is the only dependency; pinned version |
| R-X5 | 10 Success | 99.9 % of accepted events reach GreptimeDB within 5 s (age of oldest unwritten event) |
| R-X6 | 10 Phase 6 | embedded UI built only if Grafana shows a gap — go/no-go recorded |
| R-X7 | 12 ADR 0027 | licence decided before first public release (Apache-2.0) |
| R-X8 | 10 Phase 0 | synthetic generator (size configurable; evidenced at 1 M events, the 50 M run descoped); perf gates p95 rollups < 2 s, funnel/retention/sessions < 10 s |
| R-X9 | 7 | container image runs non-root, keeps the spool on a persistent volume, has a health probe, and binds the listen address from env |
| R-X10 | 10 | release image is published to GitHub Container Registry on every release tag, multi-arch, versioned and `latest` |
| R-J1 | 6, 7 step 7 | jobs run on a schedule inside `serve`: daily first-seen insert per site and hourly reconcile; also runnable once from the CLI |
| R-G18 | 5 events | optional route templates normalise high-cardinality paths (`/users/42/orders` → `/users/:id/orders`) |
| R-G19 | 11, 7 step 2 | optional GeoIP database lookup for country; the IP is read, used and discarded; never stored or logged |
| R-G20 | 4 Identity | identity tokens must have a short lifetime: `exp` further than the configured maximum is refused |
| R-T16 | 3 Rage click | rage-click thresholds (`click_count`, `threshold_px`, `timeout_ms`) and the ignore-text list are configurable |
| R-T17 | 3 Events | INP web vital is reported on hide with value in ms, only when `vitals` is listed |
| R-D4 | 9 Overview | Overview shows this week against last |
| R-O1 | 7 Guarantees | an acknowledged event survives `kill -9` of the real process and a real database outage |
| R-O2 | 3, 10 | tracker main-thread cost is measured in a real browser on a demo page (< 50 ms) |
| R-O3 | 10 | CI actions pinned by commit SHA; dependency audit runs in CI |
| R-O4 | 9, 10 | Grafana (+ Prometheus) ship in the compose stack with dashboards provisioned; product-owner walkthrough recorded |
| R-O5 | 4, 10 | read-only database user verified; fsync interval and dedup window validated under load |
| R-O6 | 10 | dependencies are kept current: major upgrades are done deliberately, one at a time, behind the full gate (`make check`, `make test-db`, audit) |
| R-O7 | 12 | a new user reaches working Grafana dashboards with one command and no edits (compose + migrate + demo data), and tutorials with screenshots stay true to the repository; the container build is split per architecture so a release always finishes |
