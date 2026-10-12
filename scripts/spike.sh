#!/usr/bin/env bash
# Phase-0 spike: generate N events into a scratch database, migrate, run the query gates.
#   scripts/spike.sh 1000000        (needs `make db-up`)
set -euo pipefail
N="${1:-1000000}"
DB="spike_$(date +%s)"
BASE="${GREPTIME_HTTP:-http://127.0.0.1:4000}"
curl -sf -XPOST "$BASE/v1/sql" --data-urlencode "sql=CREATE DATABASE $DB" >/dev/null
CFG="$(mktemp)"
cat > "$CFG" <<TOML
[greptime]
write_url = "mysql://root@127.0.0.1:4002/$DB"
flow_prefix = "${DB}_"
TOML
BIN="${BIN:-./target/release/greprum}"
"$BIN" migrate --config "$CFG"
"$BIN" gen --events "$N" --site spike --days 7 --url "mysql://root@127.0.0.1:4002/$DB"
sleep 5
"$BIN" spike --url "mysql://root@127.0.0.1:4002/$DB" --site spike --runs 5
