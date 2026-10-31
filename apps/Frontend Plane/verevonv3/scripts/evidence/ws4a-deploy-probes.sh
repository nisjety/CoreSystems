#!/usr/bin/env bash
# ws4a-deploy-probes.sh: the WS4.A deployment evidence (plan slice WS4.A, "Deployment
# evidence") and the `/metrics` read, as one probe script with four phases. A RECIPE for
# the side-by-side evidence stack: the build does not run it, and it refuses a Redis that
# does not look throwaway (the probes WRITE cache keys through the gateway).
#
# Stack needed: the rebuilt gateway (WS4.A) with GATEWAY_CACHE_REDIS_URL pointing at a
# scratch Redis, a Quarry edge with a search backend whose log you can read, org-core, and
# two seeded dev accounts in two different organizations (org A, org B) whose session cookies
# are in files. Org B must be flippable to ZDR (metadata.interactiveRetention.zdr=true) and
# org-core must be stoppable: both are operator steps between phases, not done by this script.
#
#   metrics   GET /version and /metrics: the revision is not "unverified"; the counter
#             gateway_result_cache_total exists and carries only the labels ns and outcome
#             (no org or user label); prints the counter per outcome.
#   ab        steps (1) and (2): org A posts a unique query twice, then org B posts it once.
#   zdr       step (3): run AFTER the operator set org B's ZDR posture; org B posts the
#             same query twice.
#   outage    step (4): run AFTER the operator stopped org-core; org A posts a new unique
#             query twice.
#
# Expected results (written down before any run; each phase compares deltas, so it is
# repeatable on a stack that already holds other keys):
#   ab      (1) Quarry search.upstream lines +1 after two identical posts by A; keys under
#               verevon:gw:search-web:o:<orgA>:* +1; the second post is served from cache.
#           (2) Quarry search.upstream +1 more for B; keys under o:<orgB>:* +1; the A key and
#               the B key are different strings (the pre-fix gateway had ONE key for both:
#               record that from the WS4.B forensics, not from this run).
#   zdr     (3) search.upstream +2 (no cache hit); keys under o:<orgB>:* +0;
#               gateway_result_cache_total{ns="search-web",outcome="bypass_zdr"} +2;
#               Quarry's search.result_shape lines show cache_hit false (+2 lines).
#   outage  (4) search.upstream +2; keys under o:<orgA>:* +0;
#               gateway_result_cache_total{outcome="bypass_zdr_unknown"} +2.
# A phase prints PASS or FAIL per check and exits 1 on any FAIL. NOT-RUN lines are not
# evidence.
#
# Inputs (environment). Nothing secret is on the command line: cookies are read by curl
# from files, never printed.
#   GATEWAY_URL         the side-by-side gateway, e.g. http://127.0.0.1:13185 (required)
#   REDIS_CLI           redis-cli command for the scratch Redis, e.g.
#                       "docker exec -i scratch-redis redis-cli -n 0" (required except for `metrics`)
#   QUARRY_LOG_CMD      command printing the evidence edge's log, e.g. "docker logs sbs-quarry-edge"
#   ORG_A_ID, ORG_B_ID  the two organization ids (used in the key patterns)
#   ORG_A_COOKIE_FILE, ORG_B_COOKIE_FILE   cookie files of the two seeded dev accounts
#   PUBLIC_ORIGIN       optional Origin header value (the gateway's browser-origin checks)
#   ZDR_FLIPPED_AT      for `zdr`: epoch seconds when org B's posture was set; the phase refuses
#                       to start until 61 s have passed (the posture memo, zdr.rs)
#   EVIDENCE_DIR, CURL_OPTS   as in the other probes
#
# Usage:
#   scripts/evidence/ws4a-deploy-probes.sh metrics|ab|zdr|outage
#   scripts/evidence/ws4a-deploy-probes.sh --self-test      # helpers only, no network
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"
# shellcheck source=../gateway-cache-guard.lib.sh
. "$HERE/../gateway-cache-guard.lib.sh"

# metric_value <metrics-file> <ns> <outcome>: the counter's value, 0 when absent.
metric_value() {
  awk -v ns="ns=\"$2\"" -v oc="outcome=\"$3\"" '
    /^gateway_result_cache_total\{/ && index($0, ns) && index($0, oc) { v = $NF }
    END { print (v == "" ? 0 : v + 0) }' "$1"
}

if [ "${1:-}" = "--self-test" ]; then
  tmp=$(mktemp)
  cat >"$tmp" <<'EOF'
# TYPE gateway_result_cache_total counter
gateway_result_cache_total{ns="search-web",outcome="bypass_zdr"} 2
gateway_result_cache_total{outcome="store",ns="search-web"} 5
gateway_result_cache_total{ns="scrape-preview",outcome="bypass_zdr"} 9
EOF
  ev_check "metric_value bypass_zdr (search-web)" "2" "$(metric_value "$tmp" search-web bypass_zdr)"
  ev_check "metric_value label order is irrelevant" "5" "$(metric_value "$tmp" search-web store)"
  ev_check "metric_value absent" "0" "$(metric_value "$tmp" search-web miss)"
  rm -f "$tmp"
  ev_summary
  exit $?
fi

phase=${1:-}
case "$phase" in metrics | ab | zdr | outage) ;; *) sed -n '2,12p' "${BASH_SOURCE[0]}"; echo "usage: $0 metrics|ab|zdr|outage" >&2; exit 2 ;; esac

ev_require curl node awk
: "${GATEWAY_URL:?set GATEWAY_URL to the side-by-side gateway}"
EVIDENCE_DIR=${EVIDENCE_DIR:-$(mktemp -d)}
echo "phase: $phase   evidence dir: $EVIDENCE_DIR"

fetch_metrics() { ev_http "$EVIDENCE_DIR/$1" "$GATEWAY_URL/metrics"; }

if [ "$phase" = "metrics" ]; then
  st=$(ev_http "$EVIDENCE_DIR/version.json" "$GATEWAY_URL/version")
  ev_check "/version answers" "200" "$st"
  rev=$(ev_json_get "$EVIDENCE_DIR/version.json" revision)
  [ "$rev" = "__undefined__" ] && rev=$(ev_json_get "$EVIDENCE_DIR/version.json" sourceRevision)
  case "$rev" in
    '"unverified"' | __undefined__ | __unparsable__ | '""') ev_fail "/version reports a real revision" "got $rev (the image was built without SOURCE_REVISION)" ;;
    *) ev_pass "/version reports a real revision" "$rev" ;;
  esac
  st=$(fetch_metrics metrics.txt)
  ev_check "/metrics answers" "200" "$st"
  lines=$(grep -c '^gateway_result_cache_total{' "$EVIDENCE_DIR/metrics.txt" || true)
  if [ "$lines" -ge 1 ]; then
    ev_pass "gateway_result_cache_total is exported" "$lines series"
    extra=$(grep '^gateway_result_cache_total{' "$EVIDENCE_DIR/metrics.txt" | grep -c -E 'org|user|tenant' || true)
    ev_check "the counter has no org, user or tenant label" "0" "$extra"
    for oc in hit stale_hit miss store bypass_zdr bypass_zdr_unknown bypass_no_scope bypass_request_shape scope_mismatch; do
      echo "  search-web $oc = $(metric_value "$EVIDENCE_DIR/metrics.txt" search-web "$oc")   scrape-preview $oc = $(metric_value "$EVIDENCE_DIR/metrics.txt" scrape-preview "$oc")"
    done
  else
    ev_notrun "gateway_result_cache_total is exported" "no series yet: run phase ab first (a counter appears on its first increment)"
  fi
  ev_summary
  exit $?
fi

# --- phases that post searches --------------------------------------------------------
: "${REDIS_CLI:?set REDIS_CLI to the scratch Redis (never a real one)}"
: "${QUARRY_LOG_CMD:?set QUARRY_LOG_CMD to print the evidence edge log}"
: "${ORG_A_ID:?}" "${ORG_B_ID:?}"
gateway_cache_require_throwaway "$REDIS_CLI" 0 || exit $?
LOG_CMD=$QUARRY_LOG_CMD
case "$LOG_CMD" in
  *"quarry-edge"*) echo "refusing: QUARRY_LOG_CMD names quarry-edge, the live container" >&2; exit 2 ;;
esac

count_keys() { # <org id> -> number of search-web keys of that org
  local n
  n=$($REDIS_CLI --scan --pattern "verevon:gw:search-web:o:$1:*" </dev/null | grep -c . || true)
  echo "$n"
}
first_key() { $REDIS_CLI --scan --pattern "verevon:gw:search-web:o:$1:*" </dev/null | sort | head -1; }
upstream_lines() { ev_count_log_ci 'search\.upstream'; }
shape_false_lines() { ev_count_log_ci 'search\.result_shape.*cache_hit[=":\\ ]+false|cache_hit[=":\\ ]+false.*search\.result_shape'; }

post_search() { # <cookie file> <query> <outfile> -> http status
  local origin=()
  [ -n "${PUBLIC_ORIGIN:-}" ] && origin=(-H "Origin: $PUBLIC_ORIGIN")
  ev_http "$3" -b "$1" -H 'content-type: application/json' "${origin[@]}" -X POST \
    "$GATEWAY_URL/api/v1/search/web" -d "{\"query\":\"$2\",\"includeAnswer\":true}"
}

need_cookie() { [ -r "${!1:-/nonexistent}" ] || { echo "$1 must name a readable cookie file" >&2; exit 2; }; }

q="d4-probe-$(ev_uuid)"
fetch_metrics metrics-before.txt >/dev/null

case "$phase" in
  ab)
    need_cookie ORG_A_COOKIE_FILE; need_cookie ORG_B_COOKIE_FILE
    up0=$(upstream_lines); ka0=$(count_keys "$ORG_A_ID"); kb0=$(count_keys "$ORG_B_ID")
    s1=$(post_search "$ORG_A_COOKIE_FILE" "$q" "$EVIDENCE_DIR/a1.json")
    s2=$(post_search "$ORG_A_COOKIE_FILE" "$q" "$EVIDENCE_DIR/a2.json")
    ev_check "1 org A first post: status" "200" "$s1"
    ev_check "1 org A second post: status" "200" "$s2"
    ev_check "1 search.upstream lines after two identical posts by A (delta)" "1" "$(($(upstream_lines) - up0))"
    ev_check "1 keys under o:<orgA>:* (delta)" "1" "$(($(count_keys "$ORG_A_ID") - ka0))"
    up1=$(upstream_lines)
    s3=$(post_search "$ORG_B_COOKIE_FILE" "$q" "$EVIDENCE_DIR/b1.json")
    ev_check "2 org B same query: status" "200" "$s3"
    ev_check "2 search.upstream lines for B (delta): org B did not get A's cached result" "1" "$(($(upstream_lines) - up1))"
    ev_check "2 keys under o:<orgB>:* (delta)" "1" "$(($(count_keys "$ORG_B_ID") - kb0))"
    [ "$(first_key "$ORG_A_ID")" != "$(first_key "$ORG_B_ID")" ] && ev_pass "2 the A key and the B key are different strings" "yes" ||
      ev_fail "2 the A key and the B key are different strings" "identical"
    ;;
  zdr)
    need_cookie ORG_B_COOKIE_FILE
    : "${ZDR_FLIPPED_AT:?set ZDR_FLIPPED_AT to the epoch second at which the ZDR posture of org B was set}"
    wait=$((ZDR_FLIPPED_AT + 61 - $(date +%s)))
    if [ "$wait" -gt 0 ]; then echo "the posture memo lasts 60 s: wait $wait more seconds, then run this phase again" >&2; exit 2; fi
    up0=$(upstream_lines); kb0=$(count_keys "$ORG_B_ID"); sh0=$(shape_false_lines)
    z0=$(metric_value "$EVIDENCE_DIR/metrics-before.txt" search-web bypass_zdr)
    s1=$(post_search "$ORG_B_COOKIE_FILE" "$q" "$EVIDENCE_DIR/z1.json")
    s2=$(post_search "$ORG_B_COOKIE_FILE" "$q" "$EVIDENCE_DIR/z2.json")
    ev_check "3 org B (ZDR) first post: status" "200" "$s1"
    ev_check "3 org B (ZDR) second post: status" "200" "$s2"
    ev_check "3 search.upstream lines (delta): both went upstream" "2" "$(($(upstream_lines) - up0))"
    ev_check "3 keys under o:<orgB>:* (delta): nothing was stored" "0" "$(($(count_keys "$ORG_B_ID") - kb0))"
    fetch_metrics metrics-after.txt >/dev/null
    z1=$(metric_value "$EVIDENCE_DIR/metrics-after.txt" search-web bypass_zdr)
    ev_check "3 gateway_result_cache_total{search-web,bypass_zdr} (delta)" "2" "$((z1 - z0))"
    ev_check "3 Quarry search.result_shape lines with cache_hit false (delta)" "2" "$(($(shape_false_lines) - sh0))"
    ;;
  outage)
    need_cookie ORG_A_COOKIE_FILE
    up0=$(upstream_lines); ka0=$(count_keys "$ORG_A_ID")
    u0=$(metric_value "$EVIDENCE_DIR/metrics-before.txt" search-web bypass_zdr_unknown)
    s1=$(post_search "$ORG_A_COOKIE_FILE" "$q" "$EVIDENCE_DIR/o1.json")
    s2=$(post_search "$ORG_A_COOKIE_FILE" "$q" "$EVIDENCE_DIR/o2.json")
    ev_check "4 org A with org-core stopped, first post: status (the search itself still works)" "200" "$s1"
    ev_check "4 org A with org-core stopped, second post: status" "200" "$s2"
    ev_check "4 search.upstream lines (delta): the cache was skipped both ways" "2" "$(($(upstream_lines) - up0))"
    ev_check "4 keys under o:<orgA>:* (delta): nothing was stored" "0" "$(($(count_keys "$ORG_A_ID") - ka0))"
    fetch_metrics metrics-after.txt >/dev/null
    u1=$(metric_value "$EVIDENCE_DIR/metrics-after.txt" search-web bypass_zdr_unknown)
    ev_check "4 gateway_result_cache_total{search-web,bypass_zdr_unknown} (delta)" "2" "$((u1 - u0))"
    ;;
esac

ev_summary
