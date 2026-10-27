#!/usr/bin/env bash
# verify-replica-run-control-bearers-test.sh - offline test of the bearers verify-replica-run-control.sh sends.
#
# The proof script runs against stubs placed first on PATH. The stub curl answers auth-core's token URLs, /readyz
# and the stream start from canned data, answers Stop with 500 and logs every call; the stub docker reports the four
# run-control switches as on and runs nothing. The script therefore stops at case 1 with its own FAIL line, and the
# assertions read the logged calls and the script's output. Nothing reaches a real endpoint, container or auth-core.
# The queue, feedback, plan-approval and durable-cancel calls are not reached offline, so their session bearer is
# checked against the script text instead.
#
# Usage:  bash "apps/Model Plane/scripts/tests/verify-replica-run-control-bearers-test.sh"
#         PROOF_SCRIPT=<path> runs the same checks against another copy of the proof script.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="${PROOF_SCRIPT:-$HERE/../verify-replica-run-control.sh}"
FAILS=0

# Drop the proof and auth-core variables from this shell, so a runner's own settings cannot change a case.
for v in $(compgen -v); do
  case "$v" in
    PROOF_* | MP_PROOF_* | AUTH_CORE_URL) unset "$v" ;;
  esac
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
STUB="$TMP/stub"
FAKE="$TMP/fake"
JAR="$TMP/proof-user.jar"
mkdir -p "$STUB" "$FAKE"
# The stub never reads the jar; the script only checks that it is readable and passes it to curl -b.
printf '# Netscape HTTP Cookie File\n' >"$JAR"

cat >"$STUB/curl" <<'STUB_CURL'
#!/usr/bin/env bash
# Stub curl for the bearer test: logs the call, answers from canned data, never opens a socket.
log="$FAKE_LOG"
dir="$FAKE_DIR"
printf 'CALL %s\n' "$*" >>"$log"
out_file=""
fmt=""
url=""
jar=""
args=("$@")
for ((i = 0; i < ${#args[@]}; i++)); do
  case "${args[i]}" in
    -o) out_file="${args[i + 1]:-}" ;;
    -w) fmt="${args[i + 1]:-}" ;;
    -b) jar="${args[i + 1]:-}" ;;
    http://*) url="${args[i]}" ;;
  esac
done
status=200
body=""
case "$url" in
  */api/*/token)
    aud="${url#*/api/}"
    aud="${aud%/token}"
    # FAKE_MINT_FAIL=<audience> fails every mint for that audience. FAKE_MINT_FAIL_AFTER=<n> fails session-core
    # mints once n have succeeded, so a test can put the failure on a later call, such as a Stop.
    count=0
    if [ -f "$dir/$aud" ]; then count="$(cat "$dir/$aud")"; fi
    if [ -z "$jar" ]; then
      status=401
      body='{"error":"no session"}'
    elif [ "${FAKE_MINT_FAIL:-}" = "$aud" ]; then
      status=503
      body='{"error":"boom"}'
    elif [ "$aud" = session-core ] && [ -n "${FAKE_MINT_FAIL_AFTER:-}" ] && [ "$count" -ge "$FAKE_MINT_FAIL_AFTER" ]; then
      status=503
      body='{"error":"boom"}'
    else
      count=$((count + 1))
      printf '%s' "$count" >"$dir/$aud"
      token="TOKSENT-$aud-$count"
      body="{\"token\":\"$token\",\"expiresInSeconds\":300}"
      printf 'ANSWER %s %s\n' "$aud" "$token" >>"$log"
    fi
    ;;
  */readyz) body='run_control=ok' ;;
  */v1/invoke/stream)
    body='event: connected
data: {"request_id":"RID-1","thread_id":"thr-1","run_id":"run-1","model":"claude-sonnet-5-5"}

event: chunk
data: {"request_id":"RID-1","delta":"Hello","done":false}

event: usage
data: {"request_id":"RID-1","provider_used":"anthropic"}
'
    ;;
  */cancel)
    status=500
    body='{"error":"stub"}'
    ;;
  *)
    status=404
    body='{}'
    ;;
esac
if [ -n "$out_file" ]; then printf '%s' "$body" >"$out_file"; else printf '%s' "$body"; fi
if [ -n "$fmt" ]; then printf '%b' "${fmt//'%{http_code}'/$status}"; fi
exit 0
STUB_CURL

cat >"$STUB/docker" <<'STUB_DOCKER'
#!/usr/bin/env bash
# Stub docker for the bearer test: logs the call, reports the four run-control switches as on, runs nothing.
printf 'DOCKER %s\n' "$*" >>"$FAKE_LOG"
if [ "${1:-}" = inspect ]; then
  printf '%s\n' MODEL_GATEWAY_RUN_CONTROL_BUS=true MODEL_GATEWAY_DURABLE_PLAN_GRANT=true \
    MODEL_GATEWAY_SHARED_TURN_RECORD=true MODEL_GATEWAY_SEND_RECEIPTS=true
fi
exit 0
STUB_DOCKER
chmod +x "$STUB/curl" "$STUB/docker"

# run_case <name> [VAR=value ...] runs the proof script with a clean fake state and sets RC, LOG, OUT and EVID.
run_case() {
  local name="$1"
  shift
  local dir="$TMP/run-$name"
  mkdir -p "$dir"
  LOG="$dir/calls.log"
  OUT="$dir/output.txt"
  EVID="$dir/evidence.txt"
  : >"$LOG"
  rm -rf "$FAKE" && mkdir -p "$FAKE"
  set +e
  env "$@" PATH="$STUB:$PATH" FAKE_LOG="$LOG" FAKE_DIR="$FAKE" PROOF_OUT="$EVID" \
    PROOF_A=http://stub-a.invalid:8080 PROOF_B=http://stub-b.invalid:28080 \
    bash "$SCRIPT" >"$OUT" 2>&1
  RC=$?
  set -e
  # The evidence tee is a separate process; give it a moment to flush before the files are read.
  sleep 1
}

check() { # check <label> <command...> counts a failure unless the command succeeds
  local label="$1"
  shift
  if "$@"; then
    echo "ok: $label"
  else
    echo "FAIL: $label"
    FAILS=$((FAILS + 1))
  fi
}
has() { grep -qF -- "$2" "$1"; }   # has <file> <fixed text>
lacks() { ! grep -qF -- "$2" "$1"; } # lacks <file> <fixed text>
no_calls() { [ ! -s "$LOG" ]; }
# A token mint for the audience was made with the cookie jar as curl -b.
mint_with_jar() {
  awk -v u="http://127.0.0.1:3011/api/$1/token" -v j="$JAR" \
    'index($0, "CALL ") == 1 && index($0, u) && index($0, " -b " j " ") { found = 1 } END { exit !found }' "$LOG"
}
# Every stream start carried the user, session and inference bearers minted just before it.
streams_carry_minted_bearers() {
  awk '
    /^ANSWER model-plane / { user = $3 }
    /^ANSWER session-core / { sess = $3 }
    /^ANSWER inference-core / { inf = $3 }
    /^CALL / && index($0, "/v1/invoke/stream") {
      streams++
      if (index($0, "Authorization: Bearer " user " ") && index($0, "x-session-authorization: Bearer " sess " ") && index($0, "x-inference-authorization: Bearer " inf " ")) good++
    }
    END { exit !(streams > 0 && streams == good) }' "$LOG"
}
# Stop goes out on the user token alone.
stop_sends_no_session() {
  awk '/^CALL / && index($0, "/cancel") && index($0, "x-session-authorization") { bad = 1 } END { exit bad }' "$LOG"
}
stop_route_bare() { has "$LOG" "/v1/invoke/RID-1/cancel" && stop_sends_no_session; }
no_creds_nothing_written() { [ ! -s "$LOG" ] && [ ! -e "$EVID" ]; }
direct_stream_values() {
  has "$LOG" "Authorization: Bearer DIRECT-USER-SENT" &&
    has "$LOG" "x-session-authorization: Bearer DIRECT-SESSION-SENT" &&
    has "$LOG" "x-inference-authorization: Bearer DIRECT-INFERENCE-SENT"
}
direct_values_absent() { ! grep -qF -- DIRECT- "$OUT" && ! grep -qF -- DIRECT- "$EVID"; }
script_parses() { bash -n "$SCRIPT"; }
six_session_sites() {
  local sites marked
  sites="$(grep -c '\$(http_post .*/\(queue\|feedback\|plan-approval\|orchestration/runs/\)' "$SCRIPT" || true)"
  marked="$(grep -c '\$(http_post .*/\(queue\|feedback\|plan-approval\|orchestration/runs/\).* session)"' "$SCRIPT" || true)"
  echo "    session call sites: $marked of $sites carry the session bearer"
  [ "$sites" -eq 6 ] && [ "$marked" -eq 6 ]
}
stop_and_resume_bare() {
  local stops stops_with_session resume_with_session
  stops="$(grep -c '\$(http_post .*/v1/invoke/.*/cancel"' "$SCRIPT" || true)"
  stops_with_session="$(grep -c '\$(http_post .*/v1/invoke/.*/cancel".* session)' "$SCRIPT" || true)"
  resume_with_session="$(grep -A1 'invoke/resume/' "$SCRIPT" | grep -c 'x-session' || true)"
  echo "    stop call sites: $stops, with the session bearer: $stops_with_session; resume lines with it: $resume_with_session"
  [ "$stops" -eq 5 ] && [ "$stops_with_session" -eq 0 ] && [ "$resume_with_session" -eq 0 ]
}

echo "== cookie mode: the three bearers are minted from the cookie jar and sent on the calls that take them"
run_case cookie MP_PROOF_COOKIE_JAR="$JAR"
check "cookie mode: the script exits 1" [ "$RC" -eq 1 ]
check "cookie mode: replica A passes the preflight" has "$OUT" "ok: A is served by anthropic"
check "cookie mode: replica B passes the preflight" has "$OUT" "ok: B is served by anthropic"
check "cookie mode: the run stops at case 1 with the script's own FAIL line" \
  has "$OUT" "FAIL: case 1: Stop via B answers 202: wanted '202', got '500'"
check "cookie mode: the user token is minted with the cookie jar" mint_with_jar model-plane
check "cookie mode: the session bearer is minted with the cookie jar" mint_with_jar session-core
check "cookie mode: the inference bearer is minted with the cookie jar" mint_with_jar inference-core
check "cookie mode: every stream start carries the minted user, session and inference bearers" streams_carry_minted_bearers
check "cookie mode: Stop is sent on the user token alone" stop_route_bare
check "cookie mode: no token value reaches the output" lacks "$OUT" "TOKSENT-"
check "cookie mode: no token value reaches the evidence file" lacks "$EVID" "TOKSENT-"
check "cookie mode: the evidence file was written" has "$EVID" "== preflight"

echo "== refusals: no credentials, and the proof bearer alone"
run_case none
check "no credentials: the script exits 1" [ "$RC" -eq 1 ]
check "no credentials: the refusal names the cookie jar and the three bearers" \
  has "$OUT" "FAIL: set MP_PROOF_COOKIE_JAR"
check "no credentials: nothing is called and no evidence file is written" no_creds_nothing_written
run_case bearer-only MP_PROOF_BEARER=BEARER-ONLY-SENT
check "bearer alone: the script exits 1" [ "$RC" -eq 1 ]
check "bearer alone: the refusal names the cookie jar and the three bearers" \
  has "$OUT" "FAIL: set MP_PROOF_COOKIE_JAR"
check "bearer alone: nothing is called" no_calls
check "bearer alone: the value is not echoed" lacks "$OUT" "BEARER-ONLY-SENT"

echo "== direct mode: the three values as given, nothing minted"
run_case direct MP_PROOF_BEARER=DIRECT-USER-SENT MP_PROOF_SESSION_BEARER=DIRECT-SESSION-SENT \
  MP_PROOF_INFERENCE_BEARER=DIRECT-INFERENCE-SENT
check "direct mode: the run reaches case 1" has "$OUT" "FAIL: case 1: Stop via B answers 202"
check "direct mode: no auth-core token call is made" lacks "$LOG" "/api/"
check "direct mode: the stream start carries the three values as given" direct_stream_values
check "direct mode: the values reach neither the output nor the evidence file" direct_values_absent

echo "== a failed mint names the audience and the status, never the body"
run_case mint-fail MP_PROOF_COOKIE_JAR="$JAR" FAKE_MINT_FAIL=session-core
check "failed mint: the script exits 1" [ "$RC" -eq 1 ]
check "failed mint: the FAIL line names the audience and the HTTP status" \
  has "$OUT" "FAIL: mint session-core: auth-core answered HTTP 503"
check "failed mint: the answer body is not printed" lacks "$OUT" "boom"

echo "== a mint that fails on a later call, inside a command substitution, still prints its FAIL line"
run_case mint-fail-later MP_PROOF_COOKIE_JAR="$JAR" FAKE_MINT_FAIL_AFTER=3
check "later mint failure: the script exits 1" [ "$RC" -eq 1 ]
check "later mint failure: three stream starts ran first, so the failure is on case 1's Stop" [ "$(grep -c '/v1/invoke/stream' "$LOG")" -eq 3 ]
check "later mint failure: the FAIL line names the audience and the HTTP status" \
  has "$OUT" "FAIL: mint session-core: auth-core answered HTTP 503"
check "later mint failure: the FAIL line reaches the evidence file" \
  has "$EVID" "FAIL: mint session-core: auth-core answered HTTP 503"
check "later mint failure: the answer body is not printed" lacks "$OUT" "boom"
check "later mint failure: the answer body is not written to the evidence file" lacks "$EVID" "boom"
check "later mint failure: no Stop is sent once the mint fails" lacks "$LOG" "/cancel"

echo "== an unreadable cookie jar is refused before anything is called"
run_case bad-jar MP_PROOF_COOKIE_JAR="$TMP/no-such.jar"
check "unreadable jar: the script exits 1" [ "$RC" -eq 1 ]
check "unreadable jar: the refusal says so" has "$OUT" "is not a readable cookie jar"
check "unreadable jar: nothing is called" no_calls

echo "== script text: which calls carry which bearer (the offline run never reaches these calls)"
check "the proof script parses" script_parses
check "queue, feedback, plan approval and the durable run cancel pass the session bearer" six_session_sites
check "Stop and resume go out on the user token alone" stop_and_resume_bare

echo
if [ "$FAILS" -eq 0 ]; then
  echo "PASS: the bearer checks passed"
else
  echo "FAIL: $FAILS bearer check(s) failed"
  exit 1
fi
