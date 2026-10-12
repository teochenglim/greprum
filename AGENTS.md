# AGENTS.md

GrepRUM: product analytics + RUM in one Rust binary (axum), GreptimeDB as the only dependency. Self-hosted, privacy-safe defaults (no IP, no input values, no query strings).

Docs: `PRD.md` = requirements · `ROADMAP.md` = plan · `RELEASE/vX.Y.Z.md` = release notes + ACs · `DESIGN/` = why · `docs/adr/` = decisions + spike answers · `README.md` = end users.

## Commands

- `make check` (= `fmt-check` + `clippy` + `test` + `tracker-test`) — must pass before claiming anything works. ACs live as `ac<n>_*` tests; new AC ⇒ new test + row in `RELEASE/vX.Y.Z.md`.
- `make browser-test` — real-Chrome main-thread budget; run on real hardware before a release, **not in CI** (timing on shared runners is noise).
- `make build`, `make smoke`, `make db-up` (GreptimeDB in docker), `make test-db` (integration, needs `db-up`).
- `make release VERSION=x.y.z` — user only (see below).

## Git & release caveat (important)

- **The user does ALL git commits, pushes, tags, and releases manually. The agent never runs git commit/push/tag, `make bump`, or `make release`.**
- The release command is `make release VERSION=x.y.z` (space + `VERSION=`).
- Workflow (same as `~/code/veda`): **commit first, then `make release VERSION=x.y.z`** — it amends the bump into that fresh commit, pushes `HEAD`, tags `vX.Y.Z` and pushes the tag. Run it once per commit; on an already-pushed commit it would rewrite history and the push is rejected.
- When an implementation is finished, give a 1-line commit-message summary; the user commits and releases.
- `make release` amends the VERSION bump into HEAD and pushes `origin HEAD` — it fails unless the repo has ≥1 commit AND an `origin` remote.
- `VERSION` is the source of truth; `Cargo.toml` version must match (`make version-check`, enforced in CI). Tags are `vX.Y.Z`; CI is tag-driven (`v*`) plus PRs.

## Rules

- Rust stable, edition 2024. `#![forbid(unsafe_code)]`. No `unwrap()`/`expect()` outside tests and `main`.
- New dependency ⇒ a line in `DESIGN/09_design_tech_stack_and_release.md`. Keep `Cargo.toml` small; prefer std + what is already there.
- **SQL safety (ADR 0024):** table names from a fixed `match`; every value a bound parameter; every query carries `site_id = ?` from the authorised site list; time ranges capped. Never `format!` request text into SQL. DDL in `src/migrate/` is the only place with literal SQL beyond the query builders.
- **Ingest safety:** gate (key, Origin, rate, size) runs before the body is read; caps from PRD §3 "Names and caps"; P0 events are never shed except `503` on a full spool.
- Never acknowledge (`204`) before the batch is in the spool. Never store IP, input values or query strings.
- Quote reserved words in GreptimeDB DDL/SQL (`` `path` ``, `` `release` ``) — learned in the spike (ADR 0028).
- Tracker size budget (core ≤ 3 KB gz, total ≤ 5 KB gz) is a CI gate: `make tracker-size`.
- Changed behaviour ⇒ update the `DESIGN/` or `RELEASE/` doc it touches, unprompted. New spike result or implementation choice that changes the ADR log ⇒ new/updated file in `docs/adr/`.
- Test every GreptimeDB statement against the pinned version (`GREPTIME_VERSION` in Makefile) — not just by reading docs.

## Gotchas

- GreptimeDB append-mode tables reject `DELETE` (ADR 0029): erasure relies on opaque ids + TTL.
- Flow sink tables get an extra `update_at` column; do not `SELECT *` from them.
- `sendBeacon` sends `text/plain`; `/collect` parses JSON from raw bytes, never via an `application/json` extractor.
- The binary needs a persistent volume for the spool; tests use `tempfile` dirs.
