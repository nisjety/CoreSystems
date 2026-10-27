#!/usr/bin/env bash
# verify-replica-run-control.sh — the WS04-S9 live proof for replica-safe run control.
#
# Runs six cases against a stack that is already up with two model-gateway replicas
# (deploy/docker-compose.replicas.yml: A on 127.0.0.1:8080, B on 127.0.0.1:28080) and the
# NATS container running with the run-control ACL:
#   1. Stop sent to B for a stream owned by A ends A's stream with `stopped`; B answers 202 via peer,
#      and A's active-stream gauge returns to 0.
#   2. A queued message sent to B is delivered by A at a tool-round boundary (`queued_input`); its
#      client message id is stored once, and a repeat sent to B stores no second row.
#   3. A plan approved on A grants its thread durably; a Do run on B starts under that grant, and B
#      logs the grant read.
#   4. kill -9 of A mid-stream: Stop on B answers 404, the durable run cancel is accepted, a queue
#      on B answers resend_as_new_turn, and resume on B answers from the buffer; A is restarted.
#   5. NATS stopped: Stop on B answers 503 run_control_unavailable, never 202 or 404.
#   6. A turn streamed on A is rated on B, which reads its run reference from the shared turn record.
#      It runs after case 3 and before case 4, because case 4 kills A.
#
# Every turn asks for the direct Anthropic API and nothing else: the provider is fixed to
# anthropic (never a Verevon route, a ChatGPT/Codex subscription or Azure Foundry, which registers as
# azure-anthropic), and the model is PROOF_MODEL. A preflight turn on each replica must report
# provider_used anthropic before any case runs.
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
# Usage:    MP_PROOF_COOKIE_JAR=<curl cookie jar of a signed-in proof user> ./scripts/verify-replica-run-control.sh
#           or, in direct mode, MP_PROOF_BEARER, MP_PROOF_SESSION_BEARER and MP_PROOF_INFERENCE_BEARER, all three set.
# Cookie mode is the one to use (case 3's agentic turns also need the data-plane and the other delegated bearers
# the BFF sends with a chat stream, which only cookie mode mints). It mints the user, session-core and inference-core bearers from AUTH_CORE_URL
#           (default http://127.0.0.1:3011) with curl -b and jq -r: GET /api/model-plane/token, GET
#           /api/session-core/token and GET /api/inference-core/token, each answering JSON with the token in `token`
#           (model-plane-token.controller.ts:134-203, plane-token.controller.ts:203-274). Every request that carries
#           a bearer mints all three again first, so the 300 s default ttl of the session and inference tokens cannot
#           lapse mid-run. A failed mint is a FAIL naming the audience and HTTP status; the body is never printed.
#           Direct mode uses the three values as given and never re-mints, so they must be fresh, and a run that
#           outlives their ttl fails.
# Optional: PROOF_MODEL is the Anthropic model id every turn asks for (default claude-sonnet-5-5). It may
#           use only letters, digits, '.', '_', ':' and '-', at most 64 of them. PROOF_SHORT_PROMPT is the
#           preflight and case 6 turn. PROOF_TOOL_PROMPT must make the model call a tool on the proof stack
#           (case 2 waits for a tool_call frame; the turn asks for the `tools` feature as the SPA does, and the
#           default names count_words, a local built-in that needs no network); PROOF_PLAN_PROMPT must make it
#           propose a plan (case 3's plan and Do turns ask for `agentic`, as the SPA's plan and execute turns do; case 3 waits
#           for plan_proposed). PROOF_OUT overrides the evidence file; the other PROOF_* variables are read at
#           the top of the script.
# The bearers are read from the environment or minted into shell variables; none is printed or written.
# The org must be non-ZDR and its plan must allow the Anthropic model PROOF_MODEL. A ZDR turn never registers its
# stream, so Stop never reaches the bus (http_routes.rs:7868-7886), and queue, rating, plan approval and the
# durable run cancel refuse ZDR turns (require_non_zdr_durable_mutation, zdr_posture::continuation_gate).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Two credential modes, both from the environment (see the header). The three bearer variables stay empty
# until mint_all fills them in cookie mode; direct mode checks that all three were given.
AUTH_CORE_URL="${AUTH_CORE_URL:-http://127.0.0.1:3011}"
MP_PROOF_BEARER="${MP_PROOF_BEARER:-}"
MP_PROOF_SESSION_BEARER="${MP_PROOF_SESSION_BEARER:-}"
MP_PROOF_INFERENCE_BEARER="${MP_PROOF_INFERENCE_BEARER:-}"
if [ -n "${MP_PROOF_COOKIE_JAR:-}" ]; then
  if [ ! -r "$MP_PROOF_COOKIE_JAR" ]; then
    echo "FAIL: MP_PROOF_COOKIE_JAR is set but is not a readable cookie jar" >&2
    exit 1
  fi
  if ! command -v jq >/dev/null 2>&1; then
    echo "FAIL: cookie mode needs jq on PATH to read the minted tokens" >&2
    exit 1
  fi
  CRED_MODE=cookie
elif [ -n "$MP_PROOF_BEARER" ] && [ -n "$MP_PROOF_SESSION_BEARER" ] && [ -n "$MP_PROOF_INFERENCE_BEARER" ]; then
  CRED_MODE=direct
else
  echo "FAIL: set MP_PROOF_COOKIE_JAR to a curl cookie jar of a signed-in proof user, or set all of MP_PROOF_BEARER, MP_PROOF_SESSION_BEARER and MP_PROOF_INFERENCE_BEARER" >&2
  exit 1
fi

# The owner's order: the direct Anthropic API only. This is assigned, not defaulted, so an exported
# PROOF_PROVIDER cannot move the proof to another provider.
PROOF_PROVIDER=anthropic
readonly PROOF_PROVIDER
PROOF_MODEL="${PROOF_MODEL:-claude-sonnet-5-5}"
# The model id is written into JSON bodies and the log, so refuse anything outside the id alphabet
# before the evidence file is opened or any docker or curl call is made.
MODEL_ID_RE='^[A-Za-z0-9._:-]{1,64}$'
if ! [[ "$PROOF_MODEL" =~ $MODEL_ID_RE ]]; then
  echo "FAIL: PROOF_MODEL must be 1 to 64 letters, digits, '.', '_', ':' or '-'" >&2
  exit 1
fi

A="${PROOF_A:-http://127.0.0.1:8080}"
B="${PROOF_B:-http://127.0.0.1:28080}"
A_CTR="${PROOF_A_CONTAINER:-model-plane-model-gateway-1}"
B_CTR="${PROOF_B_CONTAINER:-model-plane-model-gateway-b-1}"
NATS_CTR="${PROOF_NATS_CONTAINER:-model-plane-nats-1}"
PG_CTR="${PROOF_PG_CONTAINER:-model-plane-postgres-1}"
LONG_PROMPT="${PROOF_LONG_PROMPT:-Write a long, careful essay of about two thousand words on the history of salt, with a heading for each section.}"
TOOL_PROMPT="${PROOF_TOOL_PROMPT:-First use the count_words tool on the text: salt has been traded for thousands of years. Then write a long, careful essay on the history of salt.}"
PLAN_PROMPT="${PROOF_PLAN_PROMPT:-Propose a short plan for a one-page note on salt. Do not carry the plan out.}"
DO_PROMPT="${PROOF_DO_PROMPT:-Carry out the approved plan now and write the one-page note on salt.}"
SHORT_PROMPT="${PROOF_SHORT_PROMPT:-Say hello in one short sentence.}"
JUSTIFICATION="Owner approved this plan during the WS04-S9 live proof; the run may write the one-page note only."

for text in "$LONG_PROMPT" "$TOOL_PROMPT" "$PLAN_PROMPT" "$DO_PROMPT" "$SHORT_PROMPT"; do
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

mint_token() { # mint_token <audience path> <label> -> MINTED holds the token; the body stays in this function
  local out code body
  out="$(curl -s -b "$MP_PROOF_COOKIE_JAR" -w '\n%{http_code}' "$AUTH_CORE_URL/api/$1/token" || true)"
  code="${out##*$'\n'}"
  body="${out%$'\n'*}"
  # http_post runs mint_all inside a command substitution, which captures stdout and drops the FAIL line, so these go
  # to stderr, which the evidence tee also reads. Only the status is printed; the body never is.
  if [ "$code" != 200 ]; then fail "mint $2: auth-core answered HTTP ${code:-none}" >&2; fi
  MINTED="$(printf '%s' "$body" | jq -r '.token // empty' 2>/dev/null || true)"
  if [ -z "$MINTED" ]; then fail "mint $2: the answer carries no token" >&2; fi
}
mint_all() { # refreshes MP_PROOF_BEARER, MP_PROOF_SESSION_BEARER and MP_PROOF_INFERENCE_BEARER; cookie mode only
  [ "$CRED_MODE" = cookie ] || return 0
  mint_token model-plane "the model-plane user token"; MP_PROOF_BEARER="$MINTED"
  mint_token session-core "session-core"; MP_PROOF_SESSION_BEARER="$MINTED"
  mint_token inference-core "inference-core"; MP_PROOF_INFERENCE_BEARER="$MINTED"
  # A stream start also carries the other delegated bearers the Verevon BFF sends with a chat stream
  # (verevonv3 apps/gateway/src/domains/chat/streams.rs): case 3's agentic turns are refused with
  # data_plane_auth_required without the data-plane one. Held in an array, never printed.
  STREAM_EXTRA_HDRS=()
  local pair
  for pair in data-plane:x-data-plane-authorization ingestion:x-ingestion-authorization \
      capability-core:x-capability-authorization execution-core:x-execution-authorization cost-core:x-cost-authorization; do
    mint_token "${pair%%:*}" "${pair%%:*}"; STREAM_EXTRA_HDRS+=(-H "${pair#*:}: Bearer $MINTED")
  done
}
STREAM_EXTRA_HDRS=()
http_post() { # http_post <base> <path> [json] [session] -> prints the status; the body is in $WORK/last.body
  # The session bearer goes out only with the fourth argument "session": queue, feedback, plan approval and the
  # durable run cancel take it (http_routes.rs:8632-8637, :7433-7437, :8063, :1962). Stop takes only the user token
  # (invoke_cancel, http_routes.rs:7860-7864), so it is sent without the fourth argument.
  local base="$1" path="$2" body="${3-}" want="${4-}"
  local session_hdr=()
  mint_all
  if [ "$want" = session ]; then session_hdr=(-H "x-session-authorization: Bearer $MP_PROOF_SESSION_BEARER"); fi
  if [ -n "$body" ]; then
    curl -s -o "$WORK/last.body" -w '%{http_code}' -X POST "$base$path" \
      -H "Authorization: Bearer $MP_PROOF_BEARER" ${session_hdr[@]+"${session_hdr[@]}"} \
      -H 'Content-Type: application/json' -d "$body" || true
  else
    curl -s -o "$WORK/last.body" -w '%{http_code}' -X POST "$base$path" \
      -H "Authorization: Bearer $MP_PROOF_BEARER" ${session_hdr[@]+"${session_hdr[@]}"} || true
  fi
}
last_body() { cat "$WORK/last.body" 2>/dev/null || true; }

start_stream() { # start_stream <base> <out-file> <json> -> STREAM_PID
  mint_all
  # The stream start needs the session and inference bearers as well (auth.rs:1291-1292, sse.rs:1069-1070).
  curl -sN --max-time 900 -X POST "$1/v1/invoke/stream" \
    -H "Authorization: Bearer $MP_PROOF_BEARER" \
    -H "x-session-authorization: Bearer $MP_PROOF_SESSION_BEARER" \
    -H "x-inference-authorization: Bearer $MP_PROOF_INFERENCE_BEARER" \
    ${STREAM_EXTRA_HDRS[@]+"${STREAM_EXTRA_HDRS[@]}"} \
    -H 'Content-Type: application/json' \
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

show_model() { # show_model <file> -> the model the connected frame names
  echo "    model=$(json_field model "$(frame_data "$1" connected)")"
}

active_streams() { # active_streams <base> -> the gateway_active_streams sample as a number, or nothing
  curl -s "$1/metrics" 2>/dev/null | awk '/^gateway_active_streams / {print $2 + 0; exit}' || true
}

served_by_direct() { # served_by_direct <base> <label> -> fails unless the turn's usage frame names the direct provider
  local out="$WORK/preflight-$2.sse" P PU
  # features:["usage"] opts in to the usage frame, which carries provider_used; sse_events.rs:1191 withholds it otherwise.
  start_stream "$1" "$out" '{"content":"'"$SHORT_PROMPT"'","features":["usage"],"provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
  wait_frame "$out" connected 60 || fail "preflight: no connected frame from $2"
  show_model "$out"
  wait "$P" 2>/dev/null || true
  PU="$(json_field provider_used "$(frame_data "$out" usage)")"
  echo "    $2 usage frame: provider_used=${PU:-<none>}"
  if [ "$PU" != "$PROOF_PROVIDER" ]; then
    fail "preflight: $2 answered provider_used '${PU:-<none>}', wanted '$PROOF_PROVIDER'. Either inference-core registered azure-anthropic instead of the direct provider (fallback.rs:621-652), or D1-791 is not fixed."
  fi
  echo "    ok: $2 is served by $PROOF_PROVIDER"
}

echo "==> WS04-S9 replica run-control proof, $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "    A=$A ($A_CTR)  B=$B ($B_CTR)  nats=$NATS_CTR  evidence=$EVIDENCE"
echo "    provider=$PROOF_PROVIDER model=$PROOF_MODEL"

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

step "preflight: a short turn on each replica is served by the direct Anthropic provider"
served_by_direct "$A" A
served_by_direct "$B" B

step "case 1: Stop on B for a stream owned by A"
start_stream "$A" "$WORK/case1.sse" '{"content":"'"$LONG_PROMPT"'","provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait_frame "$WORK/case1.sse" connected 60 || fail "case 1: no connected frame on A"
show_model "$WORK/case1.sse"
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
GAUGE=""
for i in $(seq 1 15); do
  GAUGE="$(active_streams "$A")"
  if [ "$GAUGE" = "0" ]; then break; fi
  sleep 1
done
echo "    gateway_active_streams on A after the stream ended: ${GAUGE:-<no sample>}"
[ "$GAUGE" = "0" ] || fail "case 1: A's gateway_active_streams did not return to 0 within 15 s"
echo "    ok: A's active-stream gauge is back to 0"
show_metrics "$A" A
show_metrics "$B" B

step "case 2: a queued message sent to B is delivered by A"
start_stream "$A" "$WORK/case2.sse" '{"content":"'"$TOOL_PROMPT"'","features":["usage","tools"],"provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait_frame "$WORK/case2.sse" connected 60 || fail "case 2: no connected frame on A"
show_model "$WORK/case2.sse"
CONNECTED="$(frame_data "$WORK/case2.sse" connected)"
RID="$(json_field request_id "$CONNECTED")"
THREAD_ID="$(json_field thread_id "$CONNECTED")"
if [ -z "$RID" ]; then fail "case 2: the connected frame carries no request_id"; fi
wait_frame "$WORK/case2.sse" tool_call 240 || fail "case 2: no tool round on A; set PROOF_TOOL_PROMPT to a prompt that calls a tool"
CMID="ws04proof-$STAMP-$RANDOM"
QUEUE_BODY="{\"content\":\"Also say in one line who first traded salt.\",\"client_message_id\":\"$CMID\"}"
CODE="$(http_post "$B" "/v1/invoke/$RID/queue" "$QUEUE_BODY" session)"
echo "    POST B /v1/invoke/$RID/queue -> $CODE $(last_body)"
expect "$CODE" 202 "case 2: queue via B answers 202"
case "$(last_body)" in
  *'"queued":true'*) echo "    ok: the answer says queued true" ;;
  *) fail "case 2: the answer does not say queued true" ;;
esac
case "$(last_body)" in
  *'"persisted":true'*) echo "    ok: the send receipt says persisted" ;;
  *) fail "case 2: the send receipt does not say persisted" ;;
esac
# Scoped to the thread, so an id reused by an earlier run lives in another thread and cannot count here.
expect "$(psql_one "SELECT count(*) FROM messages WHERE thread_id='$THREAD_ID' AND client_message_id='$CMID'")" 1 "case 2: one stored row for the client message id"
CODE="$(http_post "$B" "/v1/invoke/$RID/queue" "$QUEUE_BODY" session)"
echo "    POST B /v1/invoke/$RID/queue again, same body -> $CODE"
expect "$(psql_one "SELECT count(*) FROM messages WHERE thread_id='$THREAD_ID' AND client_message_id='$CMID'")" 1 "case 2: the repeat stores no second row"
wait_frame "$WORK/case2.sse" queued_input 240 || fail "case 2: A did not deliver the queued message"
echo "    queued_input on A: $(frame_data "$WORK/case2.sse" queued_input)"
CODE="$(http_post "$A" "/v1/invoke/$RID/cancel")"
expect "$CODE" 202 "case 2: local Stop on A ends the stream"
wait "$P" 2>/dev/null || true
frame_summary "$WORK/case2.sse"
show_metrics "$B" B

step "case 3: plan approved on A, Do run on B under the durable grant"
CASE3_START="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
start_stream "$A" "$WORK/case3-plan.sse" '{"content":"'"$PLAN_PROMPT"'","plan_mode":true,"features":["usage","tools","agentic"],"provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait_frame "$WORK/case3-plan.sse" plan_proposed 240 || fail "case 3: no plan_proposed on A; set PROOF_PLAN_PROMPT to a prompt that proposes a plan"
PLAN_ID="$(json_field plan_id "$(frame_data "$WORK/case3-plan.sse" plan_proposed)")"
wait "$P" 2>/dev/null || true
CONNECTED="$(frame_data "$WORK/case3-plan.sse" connected)"
show_model "$WORK/case3-plan.sse"
RUN_ID="$(json_field run_id "$CONNECTED")"
THREAD_ID="$(json_field thread_id "$CONNECTED")"
if [ -z "$PLAN_ID" ] || [ -z "$RUN_ID" ] || [ -z "$THREAD_ID" ]; then
  fail "case 3: missing plan, run or thread id (plan=$PLAN_ID run=$RUN_ID thread=$THREAD_ID)"
fi
CODE="$(http_post "$A" "/v1/runs/$RUN_ID/plan-approval" "{\"decision\":\"approve\",\"plan_id\":\"$PLAN_ID\",\"granted_rung\":\"workspace_write\",\"justification\":\"$JUSTIFICATION\"}" session)"
echo "    POST A /v1/runs/$RUN_ID/plan-approval -> $CODE $(last_body)"
expect "$CODE" 200 "case 3: approval on A answers 200"
start_stream "$B" "$WORK/case3-do.sse" '{"content":"'"$DO_PROMPT"'","thread_id":"'"$THREAD_ID"'","features":["usage","tools","agentic"],"provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait "$P" 2>/dev/null || true
show_model "$WORK/case3-do.sse"
expect "$(frame_count "$WORK/case3-do.sse" error)" 0 "case 3: the Do run on B raised no error frame"
expect "$(psql_one "SELECT granted_rung || '|' || granted_plan_id FROM threads WHERE id='$THREAD_ID'")" "workspace_write|$PLAN_ID" "case 3: the grant is on the thread row in session-core"
# The exit is durable on the run row: approval moves runs.mode from plan to execute. RUN_PLAN_MODE_EXITED itself is a
# bus envelope (mp.v1.run.<id>.event, coordinator.rs exit_plan_mode); no service writes it to session-core's events
# table, so it is not looked for there (D1-801).
expect "$(psql_one "SELECT mode FROM runs WHERE id='$RUN_ID'")" "execute" "case 3: the approved run left plan mode on its durable run row"
# B logs the grant read when the Do run starts. Match that line by its thread id and rung, from the time this case started.
GRANT_LINE="$(docker logs --since "$CASE3_START" "$B_CTR" 2>&1 | grep -F 'plan grant read from session-core for the run start' | grep -F "$THREAD_ID" | grep -F 'workspace_write' | head -1 || true)"
if [ -z "$GRANT_LINE" ]; then
  fail "case 3: B logged no 'plan grant read from session-core for the run start' line for thread $THREAD_ID at workspace_write"
fi
echo "    B log: $GRANT_LINE"
show_metrics "$B" B

step "case 6: a turn streamed on A is rated on B (shared turn record)"
start_stream "$A" "$WORK/case6.sse" '{"content":"'"$SHORT_PROMPT"'","features":["usage"],"provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait_frame "$WORK/case6.sse" connected 60 || fail "case 6: no connected frame on A"
show_model "$WORK/case6.sse"
RID="$(json_field request_id "$(frame_data "$WORK/case6.sse" connected)")"
if [ -z "$RID" ]; then fail "case 6: the connected frame carries no request_id"; fi
wait "$P" 2>/dev/null || true
expect "$(json_field provider_used "$(frame_data "$WORK/case6.sse" usage)")" "$PROOF_PROVIDER" "case 6: the turn on A was served by $PROOF_PROVIDER"
CODE="$(http_post "$B" "/v1/feedback" "{\"request_id\":\"$RID\",\"rating\":\"positive\"}" session)"
echo "    POST B /v1/feedback -> $CODE $(last_body)"
case "$CODE" in
  200) echo "    ok: B rated the turn streamed on A" ;;
  409)
    case "$(last_body)" in
      *turn_not_rateable*) fail "case 6: B cannot rate a turn streamed on A (turn_not_rateable): the shared turn record (MODEL_GATEWAY_SHARED_TURN_RECORD) did not hand the run reference to B" ;;
    esac
    fail "case 6: feedback via B answered 409" ;;
  *) fail "case 6: feedback via B answered $CODE, wanted 200" ;;
esac
show_metrics "$B" B

step "case 4: kill -9 of the owner A mid-stream"
start_stream "$A" "$WORK/case4.sse" '{"content":"'"$LONG_PROMPT"'","provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait_frame "$WORK/case4.sse" connected 60 || fail "case 4: no connected frame on A"
show_model "$WORK/case4.sse"
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
CODE="$(http_post "$B" "/v1/orchestration/runs/$RUN_ID/cancel" "" session)"
echo "    POST B /v1/orchestration/runs/$RUN_ID/cancel -> $CODE $(last_body)"
case "$CODE" in
  200|202) echo "    ok: the durable run cancel is accepted" ;;
  *) fail "case 4: durable run cancel answered $CODE" ;;
esac
CODE="$(http_post "$B" "/v1/invoke/$RID/queue" '{"content":"Also note the date."}' session)"
echo "    POST B /v1/invoke/$RID/queue -> $CODE $(last_body)"
expect "$CODE" 404 "case 4: queue via B answers 404 with no owner"
case "$(last_body)" in
  *'"resend_as_new_turn":true'*) echo "    ok: the answer says resend_as_new_turn" ;;
  *) fail "case 4: the answer does not say resend_as_new_turn" ;;
esac
mint_all
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
start_stream "$A" "$WORK/case5.sse" '{"content":"'"$LONG_PROMPT"'","provider":"anthropic","model":"'"$PROOF_MODEL"'"}'; P=$STREAM_PID
wait_frame "$WORK/case5.sse" connected 60 || fail "case 5: no connected frame on A"
show_model "$WORK/case5.sse"
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
echo "PASS — the six run-control cases were recorded in $EVIDENCE"
