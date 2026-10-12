#!/usr/bin/env bash
# AC0.10.7(a): prove a read-only GreptimeDB user cannot write. Starts a throwaway GreptimeDB with a
# static user provider (admin rw, reader ro) on 127.0.0.1:24000/24002, migrates, runs the test, removes it.
set -euo pipefail
cd "$(dirname "$0")/.."
NAME=greprum-auth-db
IMG="greptime/greptimedb:${GREPTIME_VERSION:-v1.2.1}"
docker rm -f "$NAME" >/dev/null 2>&1 || true
docker run -d --name "$NAME" -p 127.0.0.1:24000:4000 -p 127.0.0.1:24002:4002 "$IMG" standalone start \
  --http-addr 0.0.0.0:4000 --rpc-bind-addr 0.0.0.0:4001 --mysql-addr 0.0.0.0:4002 \
  "--user-provider=static_user_provider:cmd:admin=adminpw,ro:reader=readerpw" >/dev/null
trap 'docker rm -f "$NAME" >/dev/null 2>&1 || true' EXIT
for _ in $(seq 1 40); do curl -sf -u admin:adminpw http://127.0.0.1:24000/health >/dev/null && break; sleep 1; done
export GREPRUM_RW_URL="mysql://admin:adminpw@127.0.0.1:24002/public"
export GREPRUM_RO_URL="mysql://reader:readerpw@127.0.0.1:24002/public"
cat > "$(mktemp -d)/c.toml" <<TOML
TOML
TMP="$(mktemp)"; printf '[greptime]\nwrite_url = "%s"\n' "$GREPRUM_RW_URL" > "$TMP"
cargo run -q -- migrate --config "$TMP"
cargo test --locked --test ac_ops ac_0_10_7_a -- --ignored --nocapture
