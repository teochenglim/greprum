# Privacy, retention and erasure

GrepRUM's defaults store **no IP address, no input values and no query strings**. Whoever owns
PDPA (or GDPR) compliance for the deploying organisation must review this before launch.

## PDPA checklist

- [ ] Purpose documented: product analytics for the site owner; no advertising use.
- [ ] Notice/consent: if `data-consent="required"` is used, nothing is stored or sent until `consent("granted")`.
- [ ] Do Not Track: `data-respect-dnt="true"` (browser) and `honour_dnt = true` (gateway) set if policy requires.
- [ ] Identifiers: `visitor_id` is a random browser id; `user_id` is an opaque id from the site's signed token (no emails).
- [ ] `ctx` values are chosen by the site and must not contain personal data.
- [ ] Element text capture: keep (cut to 50 chars) or disable with `store_text = false`; mark sensitive regions `data-private` or `data-rum-no-capture`.
- [ ] Retention period confirmed by the data owner (default 13 months, see below).
- [ ] Access: API keys are per site and stored hashed; Grafana uses a read-only database user.

## Retention

`events` carries a table `ttl` from `[greptime] ttl` (default `13 months`, to be confirmed by the
data owner). GreptimeDB normalises it (13 months is stored as `1year 1month 43m 12s`, ADR 0042).
Rollup tables keep their own retention; `EXPIRE AFTER` limits recomputation only and does not delete.

## Erasure

**Row-level erasure is not available.** `events` is an append-mode table and GreptimeDB rejects
`DELETE` on it (spike item 1, [ADR 0029](adr/0029-delete-from-append-only-table.md)). Therefore:

1. Rely on opaque ids: a `user_id` is meaningless without the site's own user table, so deleting the user on the site side de-links the data.
2. Rely on retention: data ages out with the table `ttl`.
3. For an urgent individual erasure, rotate the site's HMAC secret and shorten the TTL for the affected window, or rebuild the table without the rows (operator procedure, offline).

## What is never stored

| Item | Why |
|---|---|
| IP address | Used for nothing; country comes from a trusted proxy header |
| Input, textarea, select, password values | Never read by the tracker |
| Query strings and URL hashes | Stripped in the browser and again in the gateway |
| Referrer path/query | Only the origin leaves the browser; only the host is stored |
