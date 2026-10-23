#!/usr/bin/env bash
# Self-test for gateway-cache-forensics.sh and gateway-cache-purge.sh.
#
# Needs a SCRATCH Redis (never a real one): REDIS_CLI must reach an instance whose
# selected database is EMPTY, or the test refuses to start.
#
#   REDIS_CLI="docker exec -i scratch-redis redis-cli" \
#     bash "apps/Frontend Plane/verevonv3/scripts/tests/gateway-cache-scripts.test.sh"
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REDIS_CLI=${REDIS_CLI:?set REDIS_CLI to a scratch Redis}
export REDIS_CLI

size=$($REDIS_CLI DBSIZE | tr -d '\r' | grep -oE '[0-9]+' | head -1)
[ "$size" = "0" ] || { echo "refusing: database is not empty ($size keys)" >&2; exit 2; }

fail() { echo "FAIL: $*" >&2; exit 1; }
exists() { $REDIS_CLI EXISTS "$1" | tr -d '\r' | grep -oE '[0-9]+' | head -1; }
LEGACY='{"cached_at":1,"data":{"results":[{"provider":"tantivy_local"}],"answer":"SECRET-ANSWER-TEXT"}}'
SCOPED='{"cached_at":1,"data":{"results":[{"provider":"brave"}]}}'
H=deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef

$REDIS_CLI SET "verevon:gw:search-web:deadbeefdeadbeef" "$LEGACY" >/dev/null
$REDIS_CLI SET "verevon:gw:search-web:o:org-x:$H" "$SCOPED" >/dev/null
$REDIS_CLI SET "verevon:gw:ratelimit:interactive:org:org-x" "5" >/dev/null   # a neighbour that must survive

out=$(bash "$HERE/../gateway-cache-forensics.sh")
echo "$out" | grep -q "legacy entries (no org):        1" || fail "forensics legacy count"
echo "$out" | grep -q "org-scoped entries (new shape): 1" || fail "forensics scoped count"
echo "$out" | grep -q '"with_answer": 1' || fail "forensics answer count"
echo "$out" | grep -q '"with_tantivy_local": 1' || fail "forensics tantivy count"
echo "$out" | grep -q "deadbeef" && fail "forensics printed a key"
echo "$out" | grep -q "SECRET-ANSWER-TEXT" && fail "forensics printed payload text"

dry=$(bash "$HERE/../gateway-cache-purge.sh")
echo "$dry" | grep -q "would be removed: 1" || fail "dry run count"
[ "$(exists verevon:gw:search-web:deadbeefdeadbeef)" = "1" ] || fail "dry run removed the legacy key"

# More legacy keys than one UNLINK batch (100): all must go. A REDIS_CLI that is
# `docker exec -i` once ate the rest of the scan after the first batch.
(for i in $(seq 1 250); do echo "SET verevon:gw:scrape-preview:bulk$i v"; done) | $REDIS_CLI >/dev/null

bash "$HERE/../gateway-cache-purge.sh" --apply >/dev/null
[ "$($REDIS_CLI --scan --pattern "verevon:gw:scrape-preview:bulk*" </dev/null | wc -l | tr -d " ")" = "0" ] || fail "apply left bulk legacy keys behind"
[ "$(exists verevon:gw:search-web:deadbeefdeadbeef)" = "0" ] || fail "apply left the legacy key"
[ "$(exists "verevon:gw:search-web:o:org-x:$H")" = "1" ] || fail "apply removed the org-scoped key"
[ "$(exists verevon:gw:ratelimit:interactive:org:org-x)" = "1" ] || fail "apply removed a neighbour key"

after=$(bash "$HERE/../gateway-cache-purge.sh")
echo "$after" | grep -q "would be removed: 0" || fail "re-run is not 0"

$REDIS_CLI DEL "verevon:gw:search-web:o:org-x:$H" verevon:gw:ratelimit:interactive:org:org-x >/dev/null

# The guard: a target that does not look throwaway is refused (exit 2) before anything
# is read or written. These commands never reach a Redis.
for cli in "docker exec -i verevon-redis redis-cli" "redis-cli -h redis.prod.example"; do
  for script in gateway-cache-forensics.sh gateway-cache-purge.sh; do
    rc=0
    REDIS_CLI="$cli" bash "$HERE/../$script" >/dev/null 2>&1 || rc=$?
    [ "$rc" = "2" ] || fail "$script did not refuse a non-throwaway target ($cli): rc=$rc"
  done
done
echo "OK: forensics reports aggregates only, purge dry-run and apply behave, new-shape and neighbour keys survive"
