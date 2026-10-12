# 5. Going to production

The quickstart uses public demo keys. Before real traffic, do this checklist.

## 1. Your own config

```bash
cp greprum.example.toml greprum.toml
```

| Setting | What to do |
|---|---|
| `[[site]] key` | a new public key, e.g. `pk_` + 24 random characters |
| `[[site]] origins` | exactly the origins that embed the tracker; `/collect` rejects others |
| `[[site]] hmac_secret` | `openssl rand -hex 32`; signs identity tokens for `identify()` |
| `[[site]] timezone` | your reporting timezone (daily buckets, retention) |
| `[[api_key]] sha256` | `greprum hash-key <your-long-random-api-key>`; only the hash is stored |
| `[server] country_header` | set by your trusted proxy (e.g. `cf-ipcountry`); client IPs are never stored |
| `[greptime] ttl` | event retention (default 13 months) |

Run with it:

```bash
GREPRUM_CONFIG=./greprum.toml docker compose up -d
```

`migrate` runs again on each `up` and is idempotent.

## 2. Put a proxy in front

GrepRUM listens on `127.0.0.1:8080` by default. Terminate TLS in your reverse proxy and serve the tracker
from your own domain (for example `https://t.example.com/t.js`), then set `data-site` and the script `src`
accordingly. Forward the country header only from a proxy you trust.

## 3. Persistent volumes

| Volume | Why |
|---|---|
| `greprum-data` → `/data` | the spool: acknowledged events survive a crash or a database outage |
| `greptime-data` | the database |

A `204` from `/collect` means the batch is in the spool. It is delivered to GreptimeDB at least once and
deduplicated on `(session_id, seq)`.

## 4. A read-only database user for readers

Grafana and BI tools only need `SELECT`. Start GreptimeDB with a static user provider that has a read-only
user, point `[greptime] read_url` at it, and give Grafana's data source the same user. The throwaway
reference setup is [scripts/ro-user-test.sh](../scripts/ro-user-test.sh).

## 5. Watch the pipeline

| Signal | Where |
|---|---|
| spool fill, age of the oldest unwritten event, shed counters | Grafana **Usage**, `GET /metrics` |
| liveness | `GET /healthz`, container `HEALTHCHECK` |
| loss estimate and duplicate rate | hourly `reconcile` job |

## 6. Privacy

Defaults store no IP, no input values and no URL query strings. Details and your obligations (consent,
erasure, retention): [docs/privacy.md](../docs/privacy.md).

## 7. Upgrades

Pin the image tag (`ghcr.io/teochenglim/greprum:0.12.0`), read the release notes in
[RELEASE/](../RELEASE/README.md), then `docker compose pull && docker compose up -d`.
