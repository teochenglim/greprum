# ADR 0053 — Playground inits and serves in one process

Status: Accepted

## Context

The quickstart stack needed two one-shot containers around the gateway: `migrate` (tables + flows,
`--wait` for GreptimeDB) and, for demo data, a `seed` run gated on `condition: service_completed_successfully`.
Three GrepRUM containers per demo stack — three image pulls, three pods in Kubernetes terms — for work
one binary can do itself, and a newcomer had to know which of the two extra commands to run.

## Decision

`greprum playground` = `migrate --wait` + guarded demo seed + `serve`, one process. Compose starts it as
the only GrepRUM container (`command: ["playground", …, "--demo-data"]`). The seed is guarded twice:

* it only ever writes the `demo` site, and only when the loaded config defines that site — a production
  config swapped in via `GREPRUM_CONFIG` receives no synthetic rows;
* it only runs when the site has no events in the generated window, so a restarted container does not
  duplicate rows (append-mode tables reject `DELETE`, ADR 0029; duplicates would double every count).

The guard is a fixed statement with a bound site and a capped window (ADR 0024). Production deployments
that want no seeding at all keep `serve` (the image default) and run the migration as one ad-hoc command
(`docker compose run --rm greprum migrate …`).

## Consequences

`docker compose up -d` is a populated playground: no run-once services, nothing to sequence, `make demo`
is a plain `up`. The first start takes the seed time (~30 000 events) before the listener binds :8080;
restarts skip straight to serving. `migrate` remains a standalone subcommand for explicit init, and the
retry loop (`--wait`) is shared code between the two paths.
