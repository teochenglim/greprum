# DESIGN 08 — Test Strategy & AC Map

Principle: **every acceptance criterion in `RELEASE/vX.Y.Z.md` is enforced by a named test.** A release is
not "done" until `make check` is green and every AC row points at a test that exists. Claims of correctness
without a test do not count (PRD §10: "Claude Code must write and pass" each phase's acceptance test).

## Naming

- AC id: `AC<M>.<m>.<n>` (release v0.4.0, 3rd criterion → `AC0.4.3`).
- Rust: `fn ac_<M>_<m>_<n>_<what>`. Example: `ac_0_4_12_accepted_equals_written_plus_counted_drops`.
- Tracker (node): `it('AC0.2.4 rage click …')` in `tracker/test/*.test.mjs`.
- **Each AC needs ≥ 2 distinct tests** (a positive case and at least one negative/boundary case).
- Non-AC unit tests are named for behaviour and live beside the code (`#[cfg(test)]`).

## Layers

| # | Layer | Where | Needs | Runs in |
|---|---|---|---|---|
| L1 | Unit — pure functions: caps, name rules, URL normalise, clock fix, priority, dedup, JWT, SQL builders | `#[cfg(test)]` in `src/**` | nothing | `make test`, every PR |
| L2 | Component — router via `tower::oneshot`, real spool in a `tempfile` dir, `MemorySink` with failure injection, fake `Reader` | `tests/*.rs` | nothing | `make test`, every PR |
| L3 | Tracker — behaviour in jsdom (clicks, rage/dead, SPA routes, sampling, consent, batching, retry-same-seq) + size budget | `tracker/test/` | node | `make tracker-test`, `make tracker-size` |
| L4 | Integration — DDL/flows/inserts/queries on the **pinned** GreptimeDB (`#[ignore]`, `GREPRUM_TEST_DB`) | `tests/db_*.rs` | docker (`make db-up`) | `make test-db`, CI `integration` job |
| L5 | Fault injection — kill the writer mid-batch, truncate/corrupt a segment tail, DB outage 10 min (virtual time), full disk cap | `tests/fault_*.rs` | nothing (L2 harness) | `make test` |
| L6 | Spike / load — synthetic generator (1 M recorded; any size via `N=`), p95 gates | `greprum gen`, `scripts/spike.sh`, results in `docs/adr/` | docker, minutes–hours | manual, release gate for v0.1.0 and v0.4.0 |

## Invariants that must be property-tested (L1/L2)

1. **No silent loss:** `accepted == written + dead + counted_drops` after any sequence of
   {append, crash+replay, DB failure, recover}. Checked with a seeded random driver, 200 iterations.
2. **Never ack before durable (to OS):** `204` implies the frame is readable from the spool file.
3. **Replay idempotence:** replay after crash from checkpoint yields a superset of acknowledged events and
   no event twice *within the same instance's dedup window*.
4. **Shed order:** P2 shed before P1 before P0; P0 only `503` at full cap; shed counts by class add up.
5. **SQL safety:** for every query builder, the SQL text contains `site_id = ?`, contains no request
   substring (fuzzed with quotes, `;`, `--`, unicode), and table name ∈ fixed set.
6. **Site isolation:** a key scoped to site A gets `403`/empty for site B on every endpoint (table-driven
   over the endpoint list so a new endpoint without a test fails the build).
7. **Caps:** each PRD cap has a boundary test (cap, cap+1).
8. **Privacy:** no IP, input value or query string reaches `StoredEvent` (fuzz payloads containing them).

## Test data & determinism

- Time is injected (`received_at` is a parameter; spool/drainer take a `Clock`), never `SystemTime::now()` inside
  logic under test. Randomness (jitter, sampling in tracker tests) is seeded.
- Ports are ephemeral (`127.0.0.1:0`). No test touches the user's real spool or DB.
- Integration tests create a uniquely named `site_id` per test and never drop shared tables, so they can run
  against a long-lived dev GreptimeDB. Flow results are awaited with `ADMIN FLUSH_FLOW` (spike item 4).

## Gates

| Gate | Command | Blocks |
|---|---|---|
| PR | `make check` | merge |
| Tag | `make check` + `make test-db` + tag == `VERSION` | release build |
| Perf | `scripts/spike.sh` result recorded in ADR | v0.1.0 closing, v0.4.0 closing |
| Size | `make tracker-size` (core ≤ 3 KB gz, total ≤ 5 KB gz) | every PR touching `tracker/` |

## AC → test map

Maintained in each `RELEASE/vX.Y.Z.md` (single source). `scripts/ac-map.sh` (`make ac-map`) enforces the full chain defined in
[DESIGN/00](00_requirements_traceability.md): every R-ID cited by ≥ 1 AC, every AC cites ≥ 1 R-ID, every AC has ≥ 2 tests.
