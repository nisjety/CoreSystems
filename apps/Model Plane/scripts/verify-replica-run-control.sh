#!/usr/bin/env bash
# verify-replica-run-control.sh — the WS04-S9 live proof for replica-safe run control.
#
# Runs five cases against a stack that is already up with two model-gateway replicas
# (deploy/docker-compose.replicas.yml: A on 127.0.0.1:8080, B on 127.0.0.1:28080) and the
# NATS container running with the run-control ACL:
#   1. Stop sent to B for a stream owned by A ends A's stream with `stopped`; B answers 202 via peer.
#   2. A queued message sent to B is delivered by A at a tool-round boundary (`queued_input`).
#   3. A plan approved on A grants its thread durably; a Do run on B starts under that grant.
#   4. kill -9 of A mid-stream: Stop on B answers 404, the durable run cancel is accepted, a queue
#      on B answers resend_as_new_turn, and resume on B answers from the buffer; A is restarted.
#   5. NATS stopped: Stop on B answers 503 run_control_unavailable, never 202 or 404.
#
# Every response, frame summary and run_control_requests_total sample is written to the evidence
# file, which is the evidence the switch-flip gate looks for. This script does not edit compose:
# the four switches come from the environment the stack was started with, and the preflight
# refuses to run unless they are `true` on both replicas.
#
# Start the stack as deploy/docker-compose.replicas.yml describes (MODEL_PLANE_REPLICAS=2 ./scripts/
# compose.sh up -d model-gateway model-gateway-b), with the NATS ACL deployed first and these
# exported in the shell that runs compose:
#   MODEL_GATEWAY_RUN_CONTROL_BUS=true MODEL_GATEWAY_DURABLE_PLAN_GRANT=true
#   MODEL_GATEWAY_SHARED_TURN_RECORD=true MODEL_GATEWAY_SEND_RECEIPTS=true
# MODEL_GATEWAY_SHARED_IDEMPOTENCY stays unset (WS04-S7 is not flipped by this proof).
#
# Usage:    MP_PROOF_BEARER=<bearer of the proof user> ./scripts/verify-replica-run-control.sh
# Optional: PROOF_TOOL_PROMPT must make the model call a tool on the proof stack (case 2 waits
#           for a tool_call frame); PROOF_PLAN_PROMPT must make it propose a plan (case 3 waits for
#           plan_proposed). PROOF_OUT overrides the evidence file; the other PROOF_* variables are
#           read at the top of the script.
# The bearer is read from the environment and is never printed or written.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
: "${MP_PROOF_BEARER:?set MP_PROOF_BEARER to the bearer of the proof user (never printed)}"

A="${PROOF_A:-http://127.0.0.1:8080}"
B="${PROOF_B:-http://127.0.0.1:28080}"
A_CTR="${PROOF_A_CONTAINER:-model-plane-model-gateway-1}"
B_CTR="${PROOF_B_CONTAINER:-model-plane-model-gateway-b-1}"
NATS_CTR="${PROOF_NATS_CONTAINER:-model-plane-nats-1}"
PG_CTR="${PROOF_PG_CONTAINER:-model-plane-postgres-1}"
LONG_PROMPT="${PROOF_LONG_PROMPT:-Write a long, careful essay of about two thousand words on the history of salt, with a heading for each section.}"
TOOL_PROMPT="${PROOF_TOOL_PROMPT:-Call a tool to check the current date, then write a long, careful essay on the history of salt.}"
PLAN_PROMPT="${PROOF_PLAN_PROMPT:-Propose a short plan for a one-page note on salt. Do not carry the plan out.}"
DO_PROMPT="${PROOF_DO_PROMPT:-Carry out the approved plan now and write the one-page note on salt.}"
JUSTIFICATION="Owner approved this plan during the WS04-S9 live proof; the run may write the one-page note only."

for text in "$LONG_PROMPT" "$TOOL_PROMPT" "$PLAN_PROMPT" "$DO_PROMPT"; do
  case "$text" in
    *\"*|*\\*) echo "FAIL: prompts go into JSON unescaped; no double quotes or backslashes" >&2; exit 1 ;;
  esac
done

STAMP="$(date -u +%Y-%m-%d)"
EVIDENCE="${PROOF_OUT:-$ROOT/../../docs/compliance/evidence/live-proof-ws04-replica-run-control-$STAMP.txt}"
mkdir -p "$(dirname "$EVIDENCE")"
exec > >(tee -a "$EVIDENCE") 2>&1

WORK="$(mktemp -d)"
STREAM_PIDS=""
cleanup() {
  # The streams are background curls; stop any still running, and bring back a container a
  # failed case may have left stopped, so the stack is never left with a replica or NATS down.
  # shellcheck disable=SC2086
  [ -n "$STREAM_PIDS" ] && kill $STREAM_PIDS 2>/dev/null || true
  docker start "$A_CTR" "$NATS_CTR" >/dev/null 2>&1 || true
  rm -rf "$WORK"
}
trap cleanup EXIT

fail() { echo "FAIL: $*"; exit 1; }
step() { echo; echo "== $*"; }
expect() { # expect <actual> <wanted> <label>
  if [ "$1" != "$2" ]; then fail "$3: wanted '$2', got '$1'"; fi
  echo "    ok: $3"
}

http_post() { # http_post <base> <path> [json] -> prints the status; the body is in $WORK/last.body
  local base="$1" path="$2" body="${3-}"
  if [ -n "$body" ]; then
    curl -s -o "$WORK/last.body" -w '%{http_code}' -X POST "$base$path" \
      -H "Authorization: Bearer $MP_PROOF_BEARER" -H 'Content-Type: application/json' -d "$body" || true
  else
    curl -s -o "$WORK/last.body" -w '%{http_code}' -X POST "$base$path" \
      -H "Authorization: Bearer $MP_PROOF_BEARER" || true
  fi
}
last_body() { cat "$WORK/last.body" 2>/dev/null || true; }

start_stream() { # start_stream <base> <out-file> <json> -> STREAM_PID
  curl -sN --max-time 900 -X POST "$1/v1/invoke/stream" \
    -H "Authorization: Bearer $MP_PROOF_BEARER" -H 'Content-Type: application/json' \
    -d "$3" >"$2" 2>/dev/null &
  STREAM_PID=$!
  STREAM_PIDS="$STREAM_PIDS $STREAM_PID"
}

wait_frame() { # wait_frame <file> <event> <seconds>
  local i
  for i in $(seq 1 "$3"); do
    if grep -q "^event: $2\$" "$1" 2>/dev/null; then return 0; fi
    sleep 1
  done
  return 1
}

frame_data() { # frame_data <file> <event> -> the data line of the first such frame
  awk -v ev="event: $2" '$0==ev {f=1; next} f && /^data: / {sub(/^data: /, ""); print; exit}' "$1"
}

json_field() { # json_field <name> <json> -> the string value, or nothing
  printf '%s' "$2" | grep -o "\"$1\":\"[^\"]*\"" | head -1 | cut -d'"' -f4 || true
}

frame_count() { grep -c "^event: $2\$" "$1" 2>/dev/null || true; }

frame_summary() { grep '^event: ' "$1" 2>/dev/null | sort | uniq -c | sed 's/^/      /' || true; }

show_metrics() { # show_metrics <base> <label>
  echo "    run_control_requests_total on $2:"
  curl -s "$1/metrics" 2>/dev/null | grep '^run_control_requests_total' | sed 's/^/      /' || echo "      (no samples)"
}

ready() { # ready <base> -> 0 once /readyz reports run_control=ok
  local i
  for i in $(seq 1 120); do
    if curl -fsS "$1/readyz" 2>/dev/null | grep -q 'run_control=ok'; then return 0; fi
    sleep 1
  done
  return 1
}

switch_value() { # switch_value <container> <name> -> the value the container was started with
  docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$1" | tr -d '\r' | sed -n "s/^$2=//p" | head -1
}

psql_one() { docker exec "$PG_CTR" psql -U postgres -d session_core -tAc "$1" | tr -d '\r'; }

echo "==> WS04-S9 replica run-control proof, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "    A=$A ($A_CTR)  B=$B ($B_CTR)  nats=$NATS_CTR  evidence=$EVIDENCE"

step "preflight: switches as the containers were started, and readiness"
for ctr in "$A_CTR" "$B_CTR"; do
  for sw in MODEL_GATEWAY_RUN_CONTROL_BUS MODEL_GATEWAY_DURABLE_PLAN_GRANT MODEL_GATEWAY_SHARED_TURN_RECORD MODEL_GATEWAY_SEND_RECEIPTS; do
    v="$(switch_value "$ctr" "$sw")"
    echo "    $ctr $sw=${v:-<unset>}"
    expect "$v" true "$ctr $sw is on"
  done
  echo "    $ctr MODEL_GATEWAY_SHARED_IDEMPOTENCY=$(switch_value "$ctr" MODEL_GATEWAY_SHARED_IDEMPOTENCY || true) (not part of this proof)"
done
for base in "$A" "$B"; do
  if ready "$base"; then echo "    $base /readyz: run_control=ok"; else fail "$base is not run_control=ok"; fi
done

step "case 1: Stop on B for a stream owned by A"
start_stream "$A" "$WORK/case1.sse" "{\"content\":\"$LONG_PROMPT\"}"; P=$STREAM_PID
wait_frame "$WORK/case1.sse" connected 60 || fail "case 1: no connected frame on A"
RID="$(json_field request_id "$(frame_data "$WORK/case1.sse" connected)")"
if [ -z "$RID" ]; then fail "case 1: the connected frame carries no request_id"; fi
wait_frame "$WORK/case1.sse" chunk 120 || fail "case 1: no chunk from A"
CODE="$(http_post "$B" "/v1/invoke/$RID/cancel")"
echo "    POST B /v1/invoke/$RID/cancel -> $CODE $(last_body)"
expect "$CODE" 202 "case 1: Stop via B answers 202"
case "$(last_body)" in
  *'"via":"peer"'*) echo "    ok: the answer says via peer" ;;
  *) fail "case 1: the answer does not say via peer" ;;
esac
wait "$P" 2>/dev/null || true
expect "$(frame_count "$WORK/case1.sse" stopped)" 1 "case 1: A's stream ends with one stopped frame"
frame_summary "$WORK/case1.sse"
show_metrics "$A" A
show_metrics "$B" B

step "case 2: a queued message sent to B is delivered by A"
start_stream "$A" "$WORK/case2.sse" "{\"content\":\"$TOOL_PROMPT\"}"; P=$STREAM_PID
wait_frame "$WORK/case2.sse" connected 60 || fail "case 2: no connected frame on A"
RID="$(json_field request_id "$(frame_data "$WORK/case2.sse" connected)")"
if [ -z "$RID" ]; then fail "case 2: the connected frame carries no request_id"; fi
wait_frame "$WORK/case2.sse" tool_call 240 || fail "case 2: no tool round on A; set PROOF_TOOL_PROMPT to a prompt that calls a tool"
CODE="$(http_post "$B" "/v1/invoke/$RID/queue" '{"content":"Also say in one line who first traded salt."}')"
echo "    POST B /v1/invoke/$RID/queue -> $CODE $(last_body)"
expect "$CODE" 200 "case 2: queue via B answers 200"
case "$(last_body)" in
  *'"queued":true'*) echo "    ok: the answer says queued true" ;;
  *) fail "case 2: the answer does not say queued true" ;;
esac
wait_frame "$WORK/case2.sse" queued_input 240 || fail "case 2: A did not deliver the queued message"
echo "    queued_input on A: $(frame_data "$WORK/case2.sse" queued_input)"
CODE="$(http_post "$A" "/v1/invoke/$RID/cancel")"
expect "$CODE" 202 "case 2: local Stop on A ends the stream"
wait "$P" 2>/dev/null || true
frame_summary "$WORK/case2.sse"
show_metrics "$B" B

step "case 3: plan approved on A, Do run on B under the durable grant"
start_stream "$A" "$WORK/case3-plan.sse" "{\"content\":\"$PLAN_PROMPT\",\"plan_mode\":true}"; P=$STREAM_PID
wait_frame "$WORK/case3-plan.sse" plan_proposed 240 || fail "case 3: no plan_proposed on A; set PROOF_PLAN_PROMPT to a prompt that proposes a plan"
PLAN_ID="$(json_field plan_id "$(frame_data "$WORK/case3-plan.sse" plan_proposed)")"
wait "$P" 2>/dev/null || true
CONNECTED="$(frame_data "$WORK/case3-plan.sse" connected)"
RUN_ID="$(json_field run_id "$CONNECTED")"
THREAD_ID="$(json_field thread_id "$CONNECTED")"
if [ -z "$PLAN_ID" ] || [ -z "$RUN_ID" ] || [ -z "$THREAD_ID" ]; then
  fail "case 3: missing plan, run or thread id (plan=$PLAN_ID run=$RUN_ID thread=$THREAD_ID)"
fi
CODE="$(http_post "$A" "/v1/runs/$RUN_ID/plan-approval" "{\"decision\":\"approve\",\"plan_id\":\"$PLAN_ID\",\"granted_rung\":\"workspace_write\",\"justification\":\"$JUSTIFICATION\"}")"
echo "    POST A /v1/runs/$RUN_ID/plan-approval -> $CODE $(last_body)"
expect "$CODE" 200 "case 3: approval on A answers 200"
start_stream "$B" "$WORK/case3-do.sse" "{\"content\":\"$DO_PROMPT\",\"thread_id\":\"$THREAD_ID\"}"; P=$STREAM_PID
wait "$P" 2>/dev/null || true
expect "$(frame_count "$WORK/case3-do.sse" error)" 0 "case 3: the Do run on B raised no error frame"
expect "$(psql_one "SELECT granted_rung || '|' || granted_plan_id FROM threads WHERE id='$THREAD_ID'")" "workspace_write|$PLAN_ID" "case 3: the grant is on the thread row in session-core"
expect "$(psql_one "SELECT count(*) FROM events WHERE run_id='$RUN_ID' AND event_type='RUN_PLAN_MODE_EXITED'")" 1 "case 3: one RUN_PLAN_MODE_EXITED event for the approved run"
show_metrics "$B" B

step "case 4: kill -9 of the owner A mid-stream"
start_stream "$A" "$WORK/case4.sse" "{\"content\":\"$LONG_PROMPT\"}"; P=$STREAM_PID
wait_frame "$WORK/case4.sse" connected 60 || fail "case 4: no connected frame on A"
CONNECTED="$(frame_data "$WORK/case4.sse" connected)"
RID="$(json_field request_id "$CONNECTED")"
RUN_ID="$(json_field run_id "$CONNECTED")"
if [ -z "$RID" ] || [ -z "$RUN_ID" ]; then fail "case 4: the connected frame lacks request_id or run_id"; fi
wait_frame "$WORK/case4.sse" chunk 120 || fail "case 4: no chunk from A"
docker kill -s KILL "$A_CTR" >/dev/null
wait "$P" 2>/dev/null || true
echo "    killed $A_CTR; stopped frames on the dead stream: $(frame_count "$WORK/case4.sse" stopped)"
CODE="$(http_post "$B" "/v1/invoke/$RID/cancel")"
echo "    POST B /v1/invoke/$RID/cancel -> $CODE $(last_body)"
expect "$CODE" 404 "case 4: Stop via B answers 404 with no owner"
CODE="$(http_post "$B" "/v1/orchestration/runs/$RUN_ID/cancel")"
echo "    POST B /v1/orchestration/runs/$RUN_ID/cancel -> $CODE $(last_body)"
case "$CODE" in
  200|202) echo "    ok: the durable run cancel is accepted" ;;
  *) fail "case 4: durable run cancel answered $CODE" ;;
esac
CODE="$(http_post "$B" "/v1/invoke/$RID/queue" '{"content":"Also note the date."}')"
echo "    POST B /v1/invoke/$RID/queue -> $CODE $(last_body)"
expect "$CODE" 404 "case 4: queue via B answers 404 with no owner"
case "$(last_body)" in
  *'"resend_as_new_turn":true'*) echo "    ok: the answer says resend_as_new_turn" ;;
  *) fail "case 4: the answer does not say resend_as_new_turn" ;;
esac
curl -sN --max-time 30 -H "Authorization: Bearer $MP_PROOF_BEARER" "$B/v1/invoke/resume/$RID" \
  >"$WORK/case4-resume.sse" 2>/dev/null || true
echo "    resume on B: $(grep -c '^event: ' "$WORK/case4-resume.sse" || true) frame(s) from the buffer"
if [ "$(grep -c '^event: ' "$WORK/case4-resume.sse" || true)" -eq 0 ]; then
  fail "case 4: resume on B answered no frames from the buffer"
fi
docker start "$A_CTR" >/dev/null
if ready "$A"; then echo "    A restarted: run_control=ok"; else fail "case 4: A is not run_control=ok after restart"; fi
show_metrics "$B" B

step "case 5: NATS stopped, Stop on B answers 503"
start_stream "$A" "$WORK/case5.sse" "{\"content\":\"$LONG_PROMPT\"}"; P=$STREAM_PID
wait_frame "$WORK/case5.sse" connected 60 || fail "case 5: no connected frame on A"
RID="$(json_field request_id "$(frame_data "$WORK/case5.sse" connected)")"
if [ -z "$RID" ]; then fail "case 5: the connected frame carries no request_id"; fi
wait_frame "$WORK/case5.sse" chunk 120 || fail "case 5: no chunk from A"
docker stop "$NATS_CTR" >/dev/null
echo "    /readyz on B with NATS down: $(curl -s "$B/readyz" 2>/dev/null || true)"
CODE="$(http_post "$B" "/v1/invoke/$RID/cancel")"
echo "    POST B /v1/invoke/$RID/cancel -> $CODE $(last_body)"
expect "$CODE" 503 "case 5: Stop via B answers 503 while NATS is down"
case "$(last_body)" in
  *run_control_unavailable*) echo "    ok: the answer says run_control_unavailable" ;;
  *) fail "case 5: the answer does not say run_control_unavailable" ;;
esac
docker start "$NATS_CTR" >/dev/null
CODE="$(http_post "$A" "/v1/invoke/$RID/cancel")"
expect "$CODE" 202 "case 5: local Stop on A ends the stream"
wait "$P" 2>/dev/null || true
show_metrics "$B" B

echo
echo "PASS — the five run-control cases were recorded in $EVIDENCE"
