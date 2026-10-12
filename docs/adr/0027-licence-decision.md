# ADR 0027 — Licence decision

Status: Accepted

## Context

Choice between MIT, Apache 2.0 and AGPL-3 was required before the first public release. The PRD suggested starting from Umami's MIT tracker; the tracker was written from scratch instead.

## Decision

Apache License 2.0 for the binary, the tracker and the dashboards (decided by the owner,
2026-10-12). `LICENSE` holds the full text. The tracker is original code (it follows Umami's public `data-*` attribute
conventions only; no Umami source is copied or vendored), so no third-party notice is needed;
no AGPL-3 code is copied.

## Consequences

Includes an explicit patent grant and permits commercial use and redistribution. Contributions
are accepted under the same licence. `Cargo.toml` declares `license = "Apache-2.0"`; the
container image carries the `org.opencontainers.image.licenses` label.
