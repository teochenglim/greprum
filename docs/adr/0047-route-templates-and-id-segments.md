# ADR 0047 — Route templates and id-segment templating

Status: Accepted

## Context

PRD §5: "optional route templates". High-cardinality paths (`/users/42/orders`) explode the
rollup tables and make top-page lists useless.

## Decision

Per site, `[[site.routes]]` rules map `pattern` (`*` = one segment) to a `template`; the first match
wins. `template_ids = true` additionally replaces numeric, UUID and 16+ character hex segments with
`:id`. Both run after query/hash stripping and apply to `path` and `parent_path`, so flows group by
the template. Both are off by default.

## Consequences

The raw path is not recoverable from storage once templated. Rules are static config (no regex),
which keeps matching allocation-light and predictable.
