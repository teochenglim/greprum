#!/usr/bin/env bash
# Verify one release's ACs: every AC row has >=2 tests and they pass.
#   scripts/release-check.sh 0.4.0        (set GREPRUM_TEST_DB to include live tests)
set -uo pipefail
cd "$(dirname "$0")/.."
REL="${1:?usage: release-check.sh X.Y.Z}"
IFS=. read -r MA MI _ <<<"$REL"
PRE="ac_${MA}_${MI}_"
FILE="RELEASE/v$REL.md"
[ -f "$FILE" ] || { echo "no $FILE"; exit 2; }
echo "== v$REL: $(grep -cE '^\| AC' "$FILE") ACs"
fail=0
# AC -> tests present
for id in $(grep -oE '^\| AC[0-9]+\.[0-9]+\.[0-9]+' "$FILE" | tr -d '| '); do
  pre="ac_$(echo "${id#AC}" | tr . _)_"
  n=$(( $(grep -rhoE "fn ${pre}[a-z0-9_]+" src tests | sort -u | wc -l) + $(grep -rhE "(it|test)\(['\"]${id}[ :.]" tracker/test tracker/browser 2>/dev/null | sort -u | wc -l) ))
  printf '  %-10s %2d tests\n' "$id" "$n"; [ "$n" -ge 2 ] || fail=1
done
# run them (sum across test binaries)
sum() { awk '/^test result/ { for (i=1;i<=NF;i++) { if ($i=="passed;") p+=$(i-1); if ($i=="failed;") f+=$(i-1); if ($i=="ignored;") g+=$(i-1) } } END { printf "%d passed, %d failed, %d ignored", p, f, g; if (f>0) exit 1 }'; }
cargo test --locked -q "$PRE" 2>&1 | tee /tmp/rc_$REL.log | sum | sed 's/^/  rust (offline): /'; [ "${PIPESTATUS[2]}" = 0 ] || fail=1
echo
if [ -n "${GREPRUM_TEST_DB:-}" ]; then
  cargo test --locked -q "$PRE" -- --ignored --test-threads=1 2>&1 | sum | sed 's/^/  rust (live DB): /'; [ "${PIPESTATUS[2]}" = 0 ] || fail=1
  echo
fi
if [ "$MI" = "2" ] || [ "$MI" = "9" ] || [ "$MI" = "10" ]; then (cd tracker && node --test test/*.test.mjs browser/*.test.mjs 2>&1 | grep -E "^ℹ (tests|pass|fail|skipped)" | sed 's/^/  tracker: /'); fi
exit $fail
