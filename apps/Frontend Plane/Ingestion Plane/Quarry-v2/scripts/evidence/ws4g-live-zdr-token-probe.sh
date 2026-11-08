#!/usr/bin/env bash
# ws4g-live-zdr-token-probe.sh: the WS4.G "live stack with a real ZDR-organization token"
# check (plan slice WS4.G, Evidence). A RECIPE for the exit-gate / side-by-side stack: the
# build does not run it. Run it only against a stack whose organization is a seeded dev
# organization, never against the live stack of real tenants.
#
# What it checks, against quarry-edge directly with the bearer of a user of a ZDR
# organization (the matrix in docs/ZDR_MATRIX.md, rows crawl, batch, agent_start_profile,
# schedules):
#   1. POST /v1/crawl             -> 403, error message mentions ZDR      (Enforced row)
#   2. POST /v1/batch             -> 403, error message mentions ZDR      (Enforced row)
#   3. POST /v1/agent/runs with a persistent profile
#                                 -> 403, error message mentions ZDR      (Enforced row)
#   4. POST /v1/schedules {preset:"hourly", target_ref}
#                                 -> 200 TODAY (the documented Gap row `schedules`). When
#                                    WS4.H stage B lands this becomes 403 with
#                                    details.zdr_refusal.code == zdr_durable_mutation_forbidden;
#                                    the probe accepts either and says which one it saw.
#                                    A schedule it created is deleted again.
# Optional control, with STD_TOKEN_FILE (a user of a standard organization): the same
# persistent-profile start must not be refused with a ZDR message.
#
# The token comes from a FILE the user writes after signing in themselves (a bearer minted
# by Auth Core for the quarry audience). This script never asks for, prints, logs or stores a
# token or password: the file is copied to a 0600 header file that is removed on exit, and
# curl reads it with -H @file.
#
# Inputs (environment):
#   EDGE_URL         quarry-edge of the stack under test (required; no default, on purpose)
#   ZDR_TOKEN_FILE   file holding the ZDR-organization user bearer (required)
#   STD_TOKEN_FILE   optional, standard-organization user bearer for the control
#   EVIDENCE_DIR     where status codes and trimmed bodies are kept (default: new temp dir)
#   CURL_OPTS        extra curl options, e.g. --cacert /path/to/ca.pem
#
# Usage:
#   ./scripts/evidence/ws4g-live-zdr-token-probe.sh
#   ./scripts/evidence/ws4g-live-zdr-token-probe.sh --self-test   # helpers only, no network
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

# zdr_message_ok <json-file>: 0 when error.message mentions ZDR (case-insensitive).
zdr_message_ok() {
  local msg
  msg=$(ev_json_get "$1" error.message)
  case "$msg" in __undefined__ | __unparsable__) return 1 ;; esac
  printf '%s' "$msg" | grep -qi 'zdr'
}

if [ "${1:-}" = "--self-test" ]; then
  tmp=$(mktemp)
  echo '{"error":{"code":"FORBIDDEN","message":"durable crawl jobs are not available under ZDR"}}' >"$tmp"
  zdr_message_ok "$tmp" && ev_pass "zdr_message_ok matches" "yes" || ev_fail "zdr_message_ok matches" "no"
  echo '{"error":{"code":"FORBIDDEN","message":"nope"}}' >"$tmp"
  zdr_message_ok "$tmp" && ev_fail "zdr_message_ok rejects" "matched" || ev_pass "zdr_message_ok rejects" "yes"
  echo 'not json' >"$tmp"
  zdr_message_ok "$tmp" && ev_fail "zdr_message_ok on garbage" "matched" || ev_pass "zdr_message_ok on garbage" "no match"
  rm -f "$tmp"
  ev_summary
  exit $?
fi

ev_require curl node

: "${EDGE_URL:?set EDGE_URL to quarry-edge of the stack under test}"
: "${ZDR_TOKEN_FILE:?set ZDR_TOKEN_FILE to a file holding a ZDR-organization user bearer}"
[ -r "$ZDR_TOKEN_FILE" ] || { echo "ZDR_TOKEN_FILE is not readable" >&2; exit 2; }
EVIDENCE_DIR=${EVIDENCE_DIR:-$(mktemp -d)}

zdr_hdr=$(mktemp)
std_hdr=""
cleanup() { rm -f "$zdr_hdr" ${std_hdr:+"$std_hdr"}; }
trap cleanup EXIT
chmod 600 "$zdr_hdr"
{ printf 'authorization: Bearer '; tr -d '\r\n' <"$ZDR_TOKEN_FILE"; printf '\n'; } >"$zdr_hdr"
if [ -n "${STD_TOKEN_FILE:-}" ] && [ -r "$STD_TOKEN_FILE" ]; then
  std_hdr=$(mktemp)
  chmod 600 "$std_hdr"
  { printf 'authorization: Bearer '; tr -d '\r\n' <"$STD_TOKEN_FILE"; printf '\n'; } >"$std_hdr"
fi
json=(-H 'content-type: application/json')

echo "evidence dir: $EVIDENCE_DIR"
st=$(ev_http "$EVIDENCE_DIR/health.json" "$EDGE_URL/health")
ev_check "precondition: edge answers /health" "200" "$st"
[ "$st" = "200" ] || { ev_summary; exit 1; }

# 1. crawl
st=$(ev_http "$EVIDENCE_DIR/crawl.json" -H @"$zdr_hdr" "${json[@]}" -X POST "$EDGE_URL/v1/crawl" \
  -d '{"url":"https://example.com/","max_pages":1}')
ev_check "1 crawl as a ZDR-org user: status" "403" "$st"
zdr_message_ok "$EVIDENCE_DIR/crawl.json" && ev_pass "1 crawl: message mentions ZDR" "yes" || ev_fail "1 crawl: message mentions ZDR" "no"

# 2. batch
st=$(ev_http "$EVIDENCE_DIR/batch.json" -H @"$zdr_hdr" "${json[@]}" -X POST "$EDGE_URL/v1/batch" \
  -d '{"urls":["https://example.com/"]}')
ev_check "2 batch as a ZDR-org user: status" "403" "$st"
zdr_message_ok "$EVIDENCE_DIR/batch.json" && ev_pass "2 batch: message mentions ZDR" "yes" || ev_fail "2 batch: message mentions ZDR" "no"

# 3. persistent profile at agent start
profile_body='{"profile_id":"zdr-probe-profile","persist_profile":true}'
st=$(ev_http "$EVIDENCE_DIR/agent-profile.json" -H @"$zdr_hdr" "${json[@]}" -X POST "$EDGE_URL/v1/agent/runs" -d "$profile_body")
ev_check "3 agent start with a persistent profile as a ZDR-org user: status" "403" "$st"
zdr_message_ok "$EVIDENCE_DIR/agent-profile.json" && ev_pass "3 agent start: message mentions ZDR" "yes" || ev_fail "3 agent start: message mentions ZDR" "no"
if [ -n "$std_hdr" ]; then
  st=$(ev_http "$EVIDENCE_DIR/agent-profile-std.json" -H @"$std_hdr" "${json[@]}" -X POST "$EDGE_URL/v1/agent/runs" -d "$profile_body")
  echo "3c control (standard org), same body: HTTP $st"
  zdr_message_ok "$EVIDENCE_DIR/agent-profile-std.json" && ev_fail "3c control: a standard org is not refused for ZDR" "message mentions ZDR" ||
    ev_pass "3c control: a standard org is not refused for ZDR" "no ZDR message (status $st)"
else
  ev_notrun "3c control (standard org)" "STD_TOKEN_FILE not set"
fi

# 4. schedules: the documented gap
st=$(ev_http "$EVIDENCE_DIR/schedule.json" -H @"$zdr_hdr" "${json[@]}" -X POST "$EDGE_URL/v1/schedules" \
  -d '{"preset":"hourly","target_ref":"https://example.com/zdr-probe-schedule"}')
case "$st" in
  200)
    ev_pass "4 schedules as a ZDR-org user: the documented gap is still open" "HTTP 200 (WS4.H stage B has not landed)"
    sid=$(ev_json_get "$EVIDENCE_DIR/schedule.json" data.id | tr -d '"')
    [ "$sid" = "__undefined__" ] && sid=$(ev_json_get "$EVIDENCE_DIR/schedule.json" data.schedule_id | tr -d '"')
    if [ -n "$sid" ] && [ "$sid" != "__undefined__" ] && [ "$sid" != "__unparsable__" ]; then
      dst=$(ev_http "$EVIDENCE_DIR/schedule-delete.json" -H @"$zdr_hdr" -X DELETE "$EDGE_URL/v1/schedules/$sid")
      echo "4 cleanup: DELETE of the probe schedule answered HTTP $dst (delete must stay allowed under ZDR)"
    else
      echo "4 cleanup: the response carried no schedule id; delete the probe schedule by hand (target_ref ends with zdr-probe-schedule)" >&2
    fi ;;
  403)
    code=$(ev_json_get "$EVIDENCE_DIR/schedule.json" error.details.zdr_refusal.code)
    ev_check "4 schedules as a ZDR-org user: WS4.H stage B is on, the refusal code" '"zdr_durable_mutation_forbidden"' "$code"
    echo "4 note: the Gap row 'schedules' in ZDR_MATRIX.md can move to Enforced" ;;
  *) ev_fail "4 schedules as a ZDR-org user" "expected 200 (gap) or 403 (refused), got $st" ;;
esac

ev_summary
