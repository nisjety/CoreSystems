#!/usr/bin/env bash
# Purge the LEGACY (org-less) gateway content cache entries (WS4.B step 4).
#
# Dry run by default: counts what it would remove. --apply removes it.
#
#   REDIS_CLI="redis-cli -h <host> -a <password> -n 4" \
#     apps/Frontend\ Plane/verevonv3/scripts/gateway-cache-purge.sh [--apply]
#
# What it touches: keys matching verevon:gw:search-web:* and
# verevon:gw:scrape-preview:* that do NOT contain ":o:" (the org-scoped shape
# written by the fixed gateway). Keys are unlinked in batches of 100.
#
# What it never does: FLUSHDB / FLUSHALL. The Redis instance can be shared with
# the Data Plane and also holds rate-limit and session-validation keys, so a
# flush would sign users out and reset limits. Run it only after the fixed
# gateway (WS4.A) is on every replica, otherwise a replica still on the old
# image writes new legacy keys behind the purge.
set -euo pipefail

REDIS_CLI=${REDIS_CLI:-redis-cli}
NAMESPACES=${NAMESPACES:-"search-web scrape-preview"}
APPLY=0
[ "${1:-}" = "--apply" ] && APPLY=1

batch=()
flush() {
  [ "${#batch[@]}" -eq 0 ] && return 0
  $REDIS_CLI UNLINK "${batch[@]}" </dev/null >/dev/null
  batch=()
}

total=0
for ns in $NAMESPACES; do
  count=0
  # Read the whole scan first: a REDIS_CLI that is `docker exec -i` would otherwise
  # inherit the loop's stdin and swallow the remaining keys.
  mapfile -t scanned < <($REDIS_CLI --scan --pattern "verevon:gw:${ns}:*" </dev/null)
  for key in "${scanned[@]}"; do
    [ -z "$key" ] && continue
    case "$key" in *:o:*) continue ;; esac   # new shape: keep
    count=$((count + 1))
    if [ "$APPLY" -eq 1 ]; then
      batch+=("$key")
      if [ "${#batch[@]}" -ge 100 ]; then flush; fi
    fi
  done
  if [ "$APPLY" -eq 1 ]; then
    flush
    echo "${ns}: legacy entries removed: ${count}"
  else
    echo "${ns}: legacy entries that would be removed: ${count}"
  fi
  total=$((total + count))
done

if [ "$APPLY" -eq 1 ]; then
  echo "done: ${total} legacy entries removed. Re-run without --apply to confirm 0."
else
  echo "dry run: ${total} legacy entries would be removed. Re-run with --apply."
fi
