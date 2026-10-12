# Release plan

One file per release: `RELEASE/vX.Y.Z.md` holds scope, acceptance criteria (each mapped to a named test),
known limitations and the release checklist. Mapping to PRD §10 phases:

| Release | Theme | PRD phase | Exit gate (summary) | Depends on |
|---|---|---|---|---|
| [v0.1.0](v0.1.0.md) | Foundation & spike | 0 | spike answers in `docs/adr/`; generator; rollup p95 < 2 s, funnel/retention/sessions < 10 s (evidenced at 1 M events; 50 M run descoped) | — |
| [v0.2.0](v0.2.0.md) | Tracker | 1 | core ≤ 3 KB gz, total ≤ 5 KB; unsampled sends nothing; flush on tab close | — (parallel with 0.1) |
| [v0.3.0](v0.3.0.md) | Schema | 2 | `greprum migrate` clean on pinned GreptimeDB, idempotent | 0.1 |
| [v0.4.0](v0.4.0.md) | Gateway | 3 | no silent loss incl. crash + 10 min outage | 0.2, 0.3 |
| [v0.5.0](v0.5.0.md) | Query API | 4 | site isolation; no SQL from request text; `window_seconds` required | 0.4 |
| [v0.6.0](v0.6.0.md) | Grafana dashboards | 5 | product owner answers the two questions without SQL | 0.5 |
| [v0.7.0](v0.7.0.md) | Container image & GHCR | — (distribution) | non-root image on a volume; multi-arch image published to GHCR on tag | 0.4 |
| [v0.8.0](v0.8.0.md) | Jobs wired | 6/7 (gap) | first-seen and reconcile run inside `serve`; retention and loss gauges live | 0.7 |
| [v0.9.0](v0.9.0.md) | PRD completeness | 3/4/5/9/11 (gap) | configurable rage, INP test, route templates, GeoIP, token TTL cap, week-vs-week | 0.8 |
| [v0.10.0](v0.10.0.md) | Operational proof & supply chain | 7/10 (gap) | kill -9 and real outage, real browser, Grafana in compose, SHA pins + audit | 0.9 |
| [v1.0.0](v1.0.0.md) | Launch hardening | 6 (decision) | embedded-UI go/no-go, licence (ADR 0027), PDPA review | 0.6 |

## Per-release workflow

1. Write/refresh `RELEASE/vX.Y.Z.md` ACs **before** code (red tests first, see DESIGN/08).
2. Implement until `make check` (and `make test-db` where listed) is green.
3. Tick the checklist in the release file; update `ROADMAP.md` status and any `DESIGN/` / `docs/adr/` touched.
4. Agent hands over a one-line commit message. **User** commits, then runs `make release VERSION=x.y.z`
   (bumps `VERSION` + `Cargo.toml`, pushes, tags `vX.Y.Z`, tag push triggers CI `release.yml`).
5. CI gate (`make check`, tag == `VERSION`) → 4-target build → GitHub Release using `RELEASE/vX.Y.Z.md` as notes.

## Versioning

SemVer. `0.x` minors map to the table above; patches (`0.x.y`) are fixes with no AC change and get a short
`RELEASE/v0.x.y.md`. `1.0.0` freezes the `/collect` payload and `/api/v1/*` shapes.
