# PRD delta (as of v0.10.0)

Status per PRD section: **Done** = implemented and covered by tests · **Partial** = built, one
part missing · **Open** = not built or not proven. "Proven" means exercised on the real engine,
process or browser, not only in unit tests.

| PRD § | Requirement | Status | Detail | Release |
|---|---|---|---|---|
| 1 | One script tag, events, privacy defaults | Done | tracker + gateway tests, privacy fuzz | v0.2.0 / v0.4.0 |
| 2 | Single binary, GreptimeDB only dependency, loopback default, pinned version | Done | Docker image 11.6 MB; v1.2.1 pinned | v0.7.0 |
| 2 | `Sink` trait + `SqlxSink` | Done | | v0.4.0 |
| 3 | Core ≤ 3 KB, total ≤ 5 KB gz | Done | 2 905 B / 3 876 B (core excludes default-on rage/dead/form extras) | v0.2.0 |
| 3 | < 50 ms main-thread blocking on a demo site | Done | real Chrome: init 5.2 ms, click 0.7 ms (`make browser-test`) | v0.10.0 |
| 3 | Rage click thresholds and ignore-text list configurable | Done | `data-rage-*` attributes | v0.9.0 |
| 3 | INP web vital | Done | tested with a fake PerformanceObserver | v0.9.0 |
| 3 | Optional features only when listed | Done | | v0.2.0 |
| 4 | Short `exp` on identity tokens | Done | `max_token_ttl_secs` (default 3600) | v0.9.0 |
| 4 | Separate DB users for write and read | **Partial** | `write_url`/`read_url` supported; `make ro-user-test` written, not yet executed (ADR 0035) | v0.10.0 |
| 5 | `events` DDL, six flows, scaled columns, TTL | Done | live verified | v0.3.0 |
| 5 | Optional route templates | Done | `[[site.routes]]`, `template_ids` | v0.9.0 |
| 5 | First-seen job feeds retention | Done | scheduled in `serve`, `jobs run-once`, live verified | v0.8.0 |
| 5 | Sessions summary table job | Done (not needed) | DDL only; sessions query p95 20 ms at 1 M, gate met | — |
| 6 | Metric definitions, scaled + unscaled | Done | | v0.5.0 |
| 6/7 | Loss estimate and duplicate rate | Done | hourly reconcile sets gauges, shed subtracted | v0.8.0 |
| 7 | Spool, group commit, drainer lanes, dedup, shedding, clock fix | Done | property + fault tests | v0.4.0 |
| 7 | No acknowledged event lost on crash / DB outage | Done | real `kill -9` ×20 and 10-minute real outage: 0 lost (ADR 0048, 0049) | v0.10.0 |
| 7 | Success metric 99.9 % within 5 s | Done | real process at 2 869 events/s: max age 1.1 s (ADR 0050); beyond a few thousand events/s the single drainer is the limit | v0.10.0 |
| 7 | Metrics list | Done | all names exposed; two gauges depend on v0.8.0 | v0.4.0 |
| 8 | 14 endpoints, auth, scoping, SQL safety, caps | Done | live verified | v0.5.0 |
| 9 | Grafana dashboards for 11 screens | Done | every query executes live | v0.6.0 |
| 9 | Overview: this week against last | Done | three comparison panels, live verified | v0.9.0 |
| 9/10 | Product owner answers the two questions without SQL | Done | rendered in Grafana, walkthrough in `docs/product-owner-walkthrough.md` | v0.10.0 |
| 10 | Phase 0 spike: 10–50 M events | Descoped | 1 M evidenced, gates pass (owner decision) | — |
| 10 | Spike items 6, 9, 10, 11 | **Partial** | 9, 10, 11 Accepted (ADR 0038–0040); item 6 (read-only user) Open until `make ro-user-test` runs | v0.10.0 |
| 11 | GeoIP country | Done | optional mmdb lookup, IP discarded | v0.9.0 |
| 11 | Honour DNT, no IP/values/query strings | Done | | v0.2.0 / v0.4.0 |
| 11 | Retention 13 months confirmed; PDPA review | **Open** | human decision/review | v1.0.0 |
| 11 | Erasure | Done (documented) | not possible on append tables (ADR 0029); procedure in `docs/privacy.md` | v0.1.0 |
| 12 | ADR log, licence | Done | Apache-2.0 (ADR 0027) | v1.0.0 |
| – | CI actions pinned by SHA, dependency audit | Done | SHA pins, audit + deny job; dependency updates are manual (`make github-action-bump`, `cargo update`) | v0.10.0 |
| – | GHCR image published | **Open** | workflow written, never run; first tag push (user) | v1.0.0 |

## Release plan for the remainder

| Release | Theme | Closes |
|---|---|---|
| [v0.8.0](../RELEASE/v0.8.0.md) | Jobs wired | first-seen + reconcile in `serve`, `jobs run-once` |
| [v0.9.0](../RELEASE/v0.9.0.md) | PRD completeness | configurable rage, INP test, route templates, GeoIP, token TTL cap, week-vs-week |
| [v0.10.0](../RELEASE/v0.10.0.md) | Operational proof & supply chain | kill -9, real outage, real browser, Grafana in compose + walkthrough, SHA pins, audit, DB user/load ADRs |
| [v1.0.0](../RELEASE/v1.0.0.md) | Launch | PDPA review, retention confirmed, first publish, soak |
