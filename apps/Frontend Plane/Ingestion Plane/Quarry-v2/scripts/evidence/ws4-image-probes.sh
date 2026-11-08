#!/usr/bin/env bash
# ws4-image-probes.sh: the WS4.D and WS4.E image probes against a rebuilt quarry-edge
# (plan slices WS4.D and WS4.E, "Definition of done, evidence"). It is a RECIPE for the
# side-by-side evidence stack: the build does not run it, and it must never be pointed
# at the live quarry-edge (the script refuses a log source that names it).
#
# What it checks: a Zero Data Retention (ZDR) request leaves no URL, query or page
# marker in the edge's process log at RUST_LOG=debug (the worst case), while the
# control request (standard identity) shows the marker where the product logs it.
#   1. scrape        POST /v1/scrape         zdr:true   -> marker count 0   (WS4.D, WS4.E)
#      control       POST /v1/scrape         zdr:false  -> marker count >= 1
#   2. stream        POST /v1/scrape/stream  zdr:true   -> marker count 0   (WS4.E)
#   3. search        POST /v1/search         zdr:true, SearXNG dead
#                                                       -> query marker 0, and a
#                    `search.upstream` line with outcome error is logged   (WS4.D, WS4.E)
#   4. agent start   POST /v1/agent/runs zdr:true + navigate step
#                                                       -> marker count 0   (WS4.E)
#   5. change/latest GET /v1/change/latest?url=<marker> with a ZDR-org token
#                                                       -> marker count 0   (WS4.E)
#                    (a GET has no body flag: only a token of a ZDR organization makes
#                     it ZDR, so this one needs ZDR_TOKEN_FILE; otherwise NOT-RUN)
#   6. suggest       POST /v1/search/suggest with a ZDR-org token: records the status
#                    and the marker count; this row is a documented Gap (no owner yet,
#                    see ZDR_MATRIX.md), so the probe reports and does not assert.
#
# The edge for the run (a private container, never the live one), as in the F7 recipe in
# docs/RUNBOOKS.md "Evidence: a rebuilt edge image":
#   docker build -t qv2v/quarry-edge:evidence --build-arg SOURCE_REVISION=evidence \
#     -f deploy/compose/Dockerfile.edge .
#   docker run -d --name qv2e-ws4-edge -p 127.0.0.1:38090:8080 \
#     -e RUST_LOG=debug -e ENVIRONMENT=dev -e QUARRY_EDGE_AUTH_DEV_BYPASS=1 \
#     -e QUARRY_BROWSER_NO_SANDBOX=1 -e QUARRY_EDGE__SEARXNG_URL=http://127.0.0.1:1 \
#     qv2v/quarry-edge:evidence
# (the dev-bypass identity asserts no posture, so probes 1 to 4 ask for ZDR with the body
# flag, which can only raise the posture; probes 5 and 6 need a real ZDR-org token.)
# The edge needs outbound access to example.com for probes 1 and 2.
#
# Inputs (environment; no secret on the command line or in this file):
#   EDGE_URL          the throwaway edge, default http://127.0.0.1:38090
#   LOG_CMD           command that prints the edge's log, default "docker logs qv2e-ws4-edge"
#   BEARER            bearer for the dev-bypass edge, default dev
#   ZDR_TOKEN_FILE    optional: a file holding a ZDR-organization user token (probes 5, 6).
#                     The user signs in themselves and writes it; this script never asks
#                     for, prints, or stores a token.
#   EVIDENCE_DIR      where status codes and counts are kept (default: a new temp dir)
#   CURL_OPTS         extra curl options
#
# Usage:
#   ./scripts/evidence/ws4-image-probes.sh               # the recipe (side-by-side stack only)
#   ./scripts/evidence/ws4-image-probes.sh --self-test   # the helpers on canned data, no network
set -uo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=lib.sh
. "$HERE/lib.sh"

if [ "${1:-}" = "--self-test" ]; then
  tmp=$(mktemp)
  echo '{"a":{"b":7},"c":"x"}' >"$tmp"
  ev_check "json_get nested" "7" "$(ev_json_get "$tmp" a.b)"
  ev_check "json_get missing" "__undefined__" "$(ev_json_get "$tmp" a.z)"
  LOG_CMD="printf %s\\n a-MARK-1 b MARK-2-MARK c"
  ev_check "count_log counts lines not matches" "2" "$(ev_count_log MARK)"
  ev_check "count_log_ci" "1" "$(ev_count_log_ci '^B$')"
  ev_check_ge "ge" "2" "3"
  rm -f "$tmp"
  [ "$RESULTS_FAIL" -eq 0 ] && echo "self-test OK" || echo "self-test FAILED"
  ev_summary
  exit $?
fi

ev_require curl node

EDGE_URL=${EDGE_URL:-http://127.0.0.1:38090}
LOG_CMD=${LOG_CMD:-docker logs qv2e-ws4-edge}
BEARER=${BEARER:-dev}
EVIDENCE_DIR=${EVIDENCE_DIR:-$(mktemp -d)}

case "$LOG_CMD" in
  *"quarry-edge"*)
    echo "refusing: LOG_CMD names quarry-edge, which is the live container. Use a private edge (qv2e-*)." >&2
    exit 2 ;;
esac
case "$EDGE_URL" in
  *:8080 | *:8080/*)
    echo "refusing: EDGE_URL port 8080 is the live edge's port. Use the private edge's port." >&2
    exit 2 ;;
esac

echo "evidence dir: $EVIDENCE_DIR"

auth_dev=(-H "authorization: Bearer $BEARER" -H 'content-type: application/json')

# --- preconditions ---------------------------------------------------------------
health=$(ev_http "$EVIDENCE_DIR/health.json" "$EDGE_URL/health")
ev_check "precondition: edge answers /health" "200" "$health"
if [ "$health" != "200" ]; then
  echo "the edge is not up; nothing below would mean anything" >&2
  ev_summary
  exit 1
fi
if ! $LOG_CMD >/dev/null 2>&1; then
  echo "LOG_CMD cannot read the edge's log: $LOG_CMD" >&2
  exit 2
fi
if [ "$(ev_count_log_ci 'edge (listening|started)|listening')" -ge 1 ]; then
  ev_pass "precondition: the log source is the edge's" "a listening line is present"
else
  ev_notrun "precondition: the log source is the edge's" "no listening line found; check LOG_CMD"
fi

# RUST_LOG=debug is the worst case; read it from the container when LOG_CMD is docker logs.
case "$LOG_CMD" in
  "docker logs "*)
    ctr=${LOG_CMD#docker logs }
    ctr=${ctr%% *}
    level=$(docker inspect "$ctr" --format '{{range .Config.Env}}{{println .}}{{end}}' 2>/dev/null |
      grep -E '^RUST_LOG=' | head -1 | cut -d= -f2)
    case "$level" in
      debug* | trace*) ev_pass "precondition: RUST_LOG is the worst case" "$level" ;;
      *) ev_fail "precondition: RUST_LOG is the worst case" "expected debug or trace, got '${level:-unset}'" ;;
    esac ;;
  *) ev_notrun "precondition: RUST_LOG level" "LOG_CMD is not 'docker logs <container>', check RUST_LOG=debug by hand" ;;
esac

# --- 1 + control: scrape ---------------------------------------------------------
zid=$(ev_uuid)
sid=$(ev_uuid)
zdr_url="https://example.com/?zdr-probe-$zid"
std_url="https://example.com/?zdr-probe-$sid"

st=$(ev_http "$EVIDENCE_DIR/scrape-zdr.json" "${auth_dev[@]}" -X POST "$EDGE_URL/v1/scrape" \
  -d "{\"url\":\"$zdr_url\",\"zdr\":true}")
ev_check "1 scrape (zdr): the request succeeded, so the log has had its chance" "200" "$st"
ev_check "1 scrape (zdr): marker count in the log" "0" "$(ev_count_log "zdr-probe-$zid")"

st=$(ev_http "$EVIDENCE_DIR/scrape-std.json" "${auth_dev[@]}" -X POST "$EDGE_URL/v1/scrape" \
  -d "{\"url\":\"$std_url\",\"zdr\":false}")
ev_check "1c scrape (standard control): the request succeeded" "200" "$st"
ev_check_ge "1c scrape (standard control): marker count in the log" "1" "$(ev_count_log "zdr-probe-$sid")"

# --- 2: stream -------------------------------------------------------------------
tid=$(ev_uuid)
st=$(ev_http "$EVIDENCE_DIR/scrape-stream.sse" -N "${auth_dev[@]}" -X POST "$EDGE_URL/v1/scrape/stream" \
  -d "{\"url\":\"https://example.com/?zdr-probe-$tid\",\"zdr\":true}")
ev_check "2 scrape/stream (zdr): the stream answered" "200" "$st"
ev_check "2 scrape/stream (zdr): marker count in the log" "0" "$(ev_count_log "zdr-probe-$tid")"

# --- 3: search with the provider dead --------------------------------------------
qid=$(ev_uuid)
st=$(ev_http "$EVIDENCE_DIR/search-zdr.json" "${auth_dev[@]}" -X POST "$EDGE_URL/v1/search" \
  -d "{\"query\":\"SECRET-QUERY-$qid\",\"zdr\":true}")
echo "3 search (zdr, SearXNG dead): HTTP $st (an error status is expected: the only provider is unreachable)"
ev_check "3 search (zdr): query marker count in the log" "0" "$(ev_count_log "SECRET-QUERY-$qid")"
ev_check_ge "3 search (zdr): a search.upstream line with outcome error is logged" "1" \
  "$(ev_count_log_ci 'search\.upstream.*error|error.*search\.upstream')"

# --- 4: agent run start ----------------------------------------------------------
aid=$(ev_uuid)
st=$(ev_http "$EVIDENCE_DIR/agent-start.json" "${auth_dev[@]}" -X POST "$EDGE_URL/v1/agent/runs" -d '{"zdr":true}')
ev_check "4 agent start (zdr): the run started" "200" "$st"
run_id=$(ev_json_get "$EVIDENCE_DIR/agent-start.json" data.run_id | tr -d '"')
if [ -n "$run_id" ] && [ "$run_id" != "__undefined__" ] && [ "$run_id" != "__unparsable__" ]; then
  st=$(ev_http "$EVIDENCE_DIR/agent-step.json" "${auth_dev[@]}" -X POST "$EDGE_URL/v1/agent/runs/$run_id/step" \
    -d "{\"action\":{\"type\":\"navigate\",\"url\":\"https://example.com/?zdr-probe-$aid\"}}")
  ev_check "4 agent navigate step (zdr): the step ran" "200" "$st"
  ev_check "4 agent start + step (zdr): marker count in the log" "0" "$(ev_count_log "zdr-probe-$aid")"
  ev_http "$EVIDENCE_DIR/agent-close.json" "${auth_dev[@]}" -X DELETE "$EDGE_URL/v1/agent/runs/$run_id" >/dev/null
else
  ev_fail "4 agent start (zdr)" "no run_id in the start response"
fi

# --- 5 + 6: token-identity probes --------------------------------------------------
if [ -n "${ZDR_TOKEN_FILE:-}" ] && [ -r "$ZDR_TOKEN_FILE" ]; then
  hdr=$(mktemp)
  chmod 600 "$hdr"
  { printf 'authorization: Bearer '; tr -d '\r\n' <"$ZDR_TOKEN_FILE"; printf '\n'; } >"$hdr"
  trap 'rm -f "$hdr"' EXIT
  cid=$(ev_uuid)
  st=$(ev_http "$EVIDENCE_DIR/change-latest.json" -H @"$hdr" \
    "$EDGE_URL/v1/change/latest?url=https%3A%2F%2Fexample.com%2F%3Fzdr-probe-$cid")
  echo "5 change/latest (zdr token): HTTP $st"
  ev_check "5 change/latest (zdr token): marker count in the log (any span field, any level)" "0" \
    "$(ev_count_log "zdr-probe-$cid")"

  gid=$(ev_uuid)
  st=$(ev_http "$EVIDENCE_DIR/suggest.json" -H @"$hdr" -H 'content-type: application/json' -X POST \
    "$EDGE_URL/v1/search/suggest" -d "{\"query\":\"SECRET-SUGGEST-$gid\"}")
  echo "6 search/suggest (zdr token): HTTP $st; documented Gap row 'search_suggest' (no owner), reported not asserted"
  echo "6 search/suggest (zdr token): marker count in the edge log: $(ev_count_log "SECRET-SUGGEST-$gid")"
else
  ev_notrun "5 change/latest (zdr token)" "ZDR_TOKEN_FILE not set; a GET has no body flag, so it needs a ZDR-organization token"
  ev_notrun "6 search/suggest (zdr token)" "ZDR_TOKEN_FILE not set"
fi

echo "kept in $EVIDENCE_DIR: status files only matter for the recorded transcript; none is printed here"
ev_summary
