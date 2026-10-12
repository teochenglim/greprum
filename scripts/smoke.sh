#!/usr/bin/env bash
# End-to-end smoke without a database: gateway up -> /healthz -> /t.js -> /collect 204 -> /metrics.
set -euo pipefail
BIN="${1:-./bin/greprum}"
DIR="$(mktemp -d)"
PORT="${PORT:-18080}"
cat > "$DIR/greprum.toml" <<TOML
[server]
listen = "127.0.0.1:$PORT"
[spool]
dir = "$DIR/spool"
[greptime]
write_url = "mysql://root@127.0.0.1:1/public"
[[site]]
id = "smoke"
key = "pk_smoke"
origins = ["https://example.com"]
TOML
# The DB is unreachable on purpose: ingest must still acknowledge (spool first).
"$BIN" serve --config "$DIR/greprum.toml" >"$DIR/log" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true; rm -rf "$DIR"' EXIT
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORT/healthz" >/dev/null && break; sleep 0.1; done
curl -sf "http://127.0.0.1:$PORT/healthz" | grep -q ok
curl -sf "http://127.0.0.1:$PORT/t.js" | grep -q greprum
CODE=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/collect?k=pk_smoke" \
  -H 'Origin: https://example.com' -H 'Content-Type: text/plain' -A 'Mozilla/5.0 (Macintosh) Safari/605' \
  --data '{"visitor_id":"v","session_id":"s","events":[{"ts":'"$(($(date +%s)*1000))"',"seq":0,"name":"$pageview","path":"/"}]}')
[ "$CODE" = "204" ] || { echo "collect returned $CODE"; cat "$DIR/log"; exit 1; }
curl -sf "http://127.0.0.1:$PORT/metrics" | grep -q 'greprum_accepted_total 1'
echo "smoke OK"
