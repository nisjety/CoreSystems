#!/usr/bin/env bash
# Read-only forensics on the gateway's content result cache (WS4.B step 1).
#
# Reports AGGREGATES for the two namespaces that cached content before the
# org-scoped key (WS4.A): verevon:gw:search-web:* and verevon:gw:scrape-preview:*.
# A key with ":o:" in it is the new org-scoped shape; any other key is legacy,
# i.e. it was written without an organization and could have been served to any.
#
# It reads cached payloads to count things, but prints only numbers and provider
# names: never a key, URL, title, snippet or answer. Nothing is written.
#
#   REDIS_CLI="redis-cli -h <host> -a <password> -n 4" \
#     apps/Frontend\ Plane/verevonv3/scripts/gateway-cache-forensics.sh
#
# REDIS_CLI is the whole command including connection flags (default: redis-cli).
# For a containerised Redis: REDIS_CLI="docker exec -i <container> redis-cli -n 4".
# Needs jq. The Redis instance can be shared (rate-limit and session keys live
# next to these), so the patterns are narrow and the script never flushes.
#
# The script refuses a Redis that does not look throwaway (a scratch- or sbs- named
# container, or a loopback host; see gateway-cache-guard.lib.sh) unless
# --allow-non-throwaway is passed. That flag is the operator's explicit step on a real
# deployment (runbook step 1).
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=gateway-cache-guard.lib.sh
. "$HERE/gateway-cache-guard.lib.sh"

ALLOW_NON_THROWAWAY=0
for arg in "$@"; do
  case "$arg" in
    --allow-non-throwaway) ALLOW_NON_THROWAWAY=1 ;;
    -h | --help) sed -n '2,24p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "unknown argument: $arg (only --allow-non-throwaway)" >&2; exit 2 ;;
  esac
done

REDIS_CLI=${REDIS_CLI:-redis-cli}
NAMESPACES=${NAMESPACES:-"search-web scrape-preview"}

command -v jq >/dev/null || { echo "jq is required" >&2; exit 2; }
gateway_cache_require_throwaway "$REDIS_CLI" "$ALLOW_NON_THROWAWAY" || exit $?

for ns in $NAMESPACES; do
  legacy=0
  scoped=0
  records=$(mktemp)
  # Read the whole scan first (see gateway-cache-purge.sh): REDIS_CLI may be `docker exec -i`.
  mapfile -t scanned < <($REDIS_CLI --scan --pattern "verevon:gw:${ns}:*" </dev/null)
  for key in "${scanned[@]}"; do
    [ -z "$key" ] && continue
    case "$key" in
      *:o:*) scoped=$((scoped + 1)) ;;
      *)
        legacy=$((legacy + 1))
        # One aggregate record per legacy entry; the payload never leaves jq.
        # age is seconds since cached_at (null when the entry has none).
        $REDIS_CLI GET "$key" </dev/null | jq -c --argjson now "$(date +%s)" '
          def providers: [.data.results[]?.provider // empty];
          { age: (if (.cached_at // 0) > 0 then ($now - .cached_at) else null end),
            answer: ((.data.answer // null) != null),
            tantivy_local: (providers | any(. == "tantivy_local")),
            providers: (providers | unique) }' >>"$records" 2>/dev/null || true
        ;;
    esac
  done

  echo "== namespace ${ns}"
  echo "   org-scoped entries (new shape): ${scoped}"
  echo "   legacy entries (no org):        ${legacy}"
  if [ "$legacy" -gt 0 ]; then
    jq -s '{
      legacy_entries: length,
      with_answer: (map(select(.answer)) | length),
      with_tantivy_local: (map(select(.tantivy_local)) | length),
      age_under_1h: (map(select(.age != null and .age < 3600)) | length),
      age_1h_to_6h: (map(select(.age != null and .age >= 3600 and .age < 21600)) | length),
      age_6h_to_24h: (map(select(.age != null and .age >= 21600 and .age < 86400)) | length),
      age_over_24h_or_unknown: (map(select(.age == null or .age >= 86400)) | length),
      providers_seen: (map(.providers) | add // [] | unique)
    }' "$records"
  fi
  rm -f "$records"
done

echo
echo "Hit rate (coarse; the instance is shared with other key families):"
$REDIS_CLI INFO stats </dev/null | tr -d '\r' | grep -E '^(keyspace_hits|keyspace_misses|total_commands_processed):' || true
