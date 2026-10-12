# ADR 0046 — Optional GeoIP lookup, IP discarded

Status: Accepted

## Context

PRD §11: the IP is used for GeoIP country and a short-lived rate-limit hash, then discarded.
Until now country came only from a trusted-proxy header (`cf-ipcountry`).

## Decision

`[geoip] database` points at a MaxMind-format country database. When set and the request has no
country header, the gateway reads the client IP (socket peer, or the first address of
`server.ip_header`), looks up the ISO code and drops the IP. A trusted-proxy country header wins.
Private, loopback and unparsable addresses give an empty country. An unreadable database disables
the lookup with a warning; the gateway still starts. The rate limit stays per site key and uses no
IP at all, which is stricter than the PRD's short-lived hash.

## Consequences

The IP exists only in a local variable inside the request handler (verified by a spool test).
Operators supply the database (licence terms of the provider apply). `maxminddb` is a new dependency.
