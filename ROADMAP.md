# GrepRUM Roadmap

> **Product analytics and RUM in one Rust binary, on GreptimeDB.**
> Replaces PostHog + ClickHouse + Kafka + Postgres for teams that want clicks, journeys, funnels and retention
> without running that stack. Privacy-safe by default.

Status: ✅ shipped · 🚧 in progress · 📋 planned · 🔭 exploratory

PRD gap list: [docs/prd-delta.md](docs/prd-delta.md)

Traceability: [PRD](PRD.md) → [requirements](DESIGN/00_requirements_traceability.md) → ACs in
[RELEASE/](RELEASE/README.md) → ≥ 2 tests each (`make ac-map`).

## v0.1.0 — Foundation & spike ✅

- [x] Cargo workspace, Makefile, VERSION, CI + tag-driven release workflow, AGENTS.md
- [x] Config, model, metrics
- [x] Synthetic generator (`greprum gen`) and spike harness (`greprum spike`)
- [x] 11 spike ADRs; 8 answered, 3 Open for their own reasons (ADR 0035 DB users, 0038 fsync/dedup under load, 0039 noisy table) and 0040 decided in v0.6.0
- [x] 1 M-event run: all gates pass (ADR 0043); the 50 M run was descoped by the owner

Details: [RELEASE/v0.1.0.md](RELEASE/v0.1.0.md)

## v0.2.0 — Tracker ✅ (real-browser profile pending)

- [x] Session start, page views, SPA routes, clicks, form submits, rage/dead clicks
- [x] Optional scroll, errors, INP, pageleave via `data-features`
- [x] Identity, consent, sampling, cookie mode, delivery with `seq`/`sent_at`/retry
- [x] Core 2.9 KB gz, total 3.9 KB gz (CI gate), 55 tests
- [ ] Demo-page main-thread profile in a real browser

Details: [RELEASE/v0.2.0.md](RELEASE/v0.2.0.md)

## v0.3.0 — Schema ✅

- [x] `events` table and six flows; `greprum migrate` idempotent; TTL; job tables; flow prefix
- [x] Verified on GreptimeDB v1.2.1 (rollups incl. scaled counts, first-seen job)

Details: [RELEASE/v0.3.0.md](RELEASE/v0.3.0.md)

## v0.4.0 — Gateway ✅ (real-DB load test pending)

- [x] `/t.js`, `/collect`, gate, caps, enrichment, identity, clock fix, dedup, shed
- [x] Spool (group commit, CRC frames), drainer lanes, checkpoint, dead frames, `Sink`
- [x] Metrics, usage, reconcile job; property + crash + 10-min-outage tests
- [x] Starts and acknowledges with the database down (`make smoke`)
- [ ] Real-DB load test numbers

Details: [RELEASE/v0.4.0.md](RELEASE/v0.4.0.md)

## v0.5.0 — Query API ✅

- [x] 14 endpoints, API keys, site scoping, SQL safety, time caps, scaled counts
- [x] Every endpoint executed on the real engine
- [x] p95 recorded at 1 M events (ADR 0043)

Details: [RELEASE/v0.5.0.md](RELEASE/v0.5.0.md)

## v0.6.0 — Grafana dashboards ✅ (walkthrough pending)

- [x] 11 screens as provisioned dashboards; every query executes on GreptimeDB
- [x] Gap analysis: only the journey diagram is a gap (no embedded UI, ADR 0044)
- [ ] Screenshots and product-owner walkthrough

Details: [RELEASE/v0.6.0.md](RELEASE/v0.6.0.md)

## v0.7.0 — Container image & GHCR release ✅ (publish on first tag)

- [x] Multi-stage non-root `Dockerfile` (11.6 MB), spool volume, binary healthcheck
- [x] `docker.yml`: multi-arch image to `ghcr.io/teochenglim/greprum` on `v*` tags
- [x] `docker-compose.yml`, env overrides, `make docker-*`
- [ ] First tag push publishes the package (user runs `make release VERSION=…`)

Details: [RELEASE/v0.7.0.md](RELEASE/v0.7.0.md)

## v0.8.0 — Jobs wired ✅

- [x] First-seen job scheduled in `serve` (retention works without manual SQL)
- [x] Hourly reconcile sets the loss and duplicate-rate gauges (shed subtracted)
- [x] `greprum jobs run-once`, job metrics
- [x] CI fix folded in: tracker deps installed with `npm ci`, retired macOS runner replaced (the `v0.7.0` tag build failed on a clean checkout)

Details: [RELEASE/v0.8.0.md](RELEASE/v0.8.0.md)

## v0.9.0 — PRD completeness ✅

- [x] Configurable rage thresholds and ignore text; INP tested
- [x] Route templates; optional GeoIP database; identity token TTL cap
- [x] Overview: this week against last

Details: [RELEASE/v0.9.0.md](RELEASE/v0.9.0.md)

## v0.10.0 — Operational proof & supply chain ✅ (read-only-user run pending)

- [x] `kill -9` (20 rounds) and real 10-minute outage: nothing acknowledged lost
- [x] Real-browser main-thread budget on a demo page
- [x] Grafana + Prometheus in compose, 11 screenshots (`images/`), product-owner walkthrough
- [x] Actions pinned by SHA, `cargo audit`/`deny`; sustained-load and fsync/dedup ADRs closed
- [ ] `make ro-user-test` (read-only DB user) executed
- [x] CI fixes folded in: Node 22 test globs, audit ignore (ADR 0051), `test-db` split, Dependabot removed (updates are manual, with tests)

Details: [RELEASE/v0.10.0.md](RELEASE/v0.10.0.md)

## v0.11.0 — Dependency upgrades ✅ (action majors verified on the tag run)

- [x] axum 0.8 (tower-http removed), sqlx 0.9 (drops `rsa`: advisory ignore removed), tracker toolchain
- [x] jsonwebtoken deliberately stays on 9.x (ADR 0052)
- [x] Action majors re-pinned by SHA

Details: [RELEASE/v0.11.0.md](RELEASE/v0.11.0.md)

## v0.11.1 — Patch: browser budget on slow CI ✅

- [x] Real-browser main-thread budget measured from `max(t0, responseEnd)` and normalised by a CPU calibration
- [ ] Green CI/Release/Docker run on the tag (user)

Details: [RELEASE/v0.11.1.md](RELEASE/v0.11.1.md)

## v1.0.0 — Launch hardening 📋

- [x] Embedded UI go/no-go (no-go, ADR 0044), licence Apache-2.0 (ADR 0027), PDPA/erasure docs, contract freeze tests
- [ ] Soak run on real hardware; close the earlier "pending" items

Details: [RELEASE/v1.0.0.md](RELEASE/v1.0.0.md)

## Later 🔭

- Native GreptimeDB ingester behind the `Sink` trait (ADR 0018)
- MaxMind GeoIP database lookup (today: trusted-proxy country header)
- Second table with shorter TTL for noisy events (spike item 10)
