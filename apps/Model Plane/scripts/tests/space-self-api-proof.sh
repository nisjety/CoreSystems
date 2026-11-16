#!/usr/bin/env bash
set -euo pipefail

# WS18-S10 proof for the agent self-API on Space actions: run-bound rename and pin on the run owner's
# thread (post_card only after WS18-S7, and only once its row is in the registry). Modelled on
# tickets-create-proof.sh and agent-action-admission-proof.sh. Written by the slice that added it;
# NOT run by that slice.
#
# Modes (first argument):
#   source   layers 1 and 2 only. This is not the WS18-S10 proof, and the output says so.
#   full     (default) layers 1 and 2, then the live phases named in PROOF_PHASES (default: flow).
#
# Layer 1, source: the registry golden and its vendored copies; the unit tests of Control, Capability Core,
#   Session Core, mp-contracts, execution-core, model-gateway, the gateway and Convex; the SPA suites; the
#   capability promotion ledger test.
# Layer 2, postgres: the Session Core Postgres tests (the #[ignore] tests) through the throwaway-container
#   harness scripts/tests/session-core-db-tests.sh, and the Control decision Postgres test in a second
#   throwaway container. Both containers are removed on exit.
# Layer 3, live (full mode): a proof stack the operator started with the switches in
#   apps/Model Plane/docs/runbooks/space-self-api-rollout.md. A scripted model proposes the tools; the calls
#   take real hops through the gateway, Control, capability-core, execution-core and session-core.
#   Phases: flow (default), tokens, allowlist-empty, zdr. A phase is recorded in phases.txt only when it passes.
#
# Fail-closed rules: a step that runs nothing, skips a test or reports zero passed is BLOCKED, never PASS.
#   A missing input is BLOCKED and the script names the input, never its value. Negatives prove absence
#   only after the bounded wait PROOF_WAIT_SECONDS (default 90); they are not run-completion checks.
#
# Live inputs (names only; the values are never printed):
#   PROOF_GATEWAY_URL            the gateway (BFF) base URL of the proof stack
#   PROOF_SESSION_DB_DSN         the proof stack's session-core database; every query here is a SELECT
#   PROOF_EVIDENCE_DIR           where evidence is written; must be outside the repository
#   PROOF_ORG_ID, PROOF_SPACE_REF, PROOF_BINDING_REF, PROOF_AGENT_SUBJECT_ID
#                                the org, the room, the agent's binding and the agent's subject id
#   PROOF_OWNER_TOKEN            the room owner's bearer token (a user token, never a service token)
#   PROOF_VIEWER_TOKEN           a viewer's bearer token in the same room
#   PROOF_SCRIPTED_TITLE         the title the scripted model proposes (letters, digits and spaces only)
#   PROOF_WAIT_SECONDS           bounded wait for negatives (default 90)
#   PROOF_THREAD_ID, PROOF_RUN_ID  read from flow-ids.txt in PROOF_EVIDENCE_DIR by the operator for the later phases
#   PROOF_DECISION_TOKEN_FILE    tokens phase: one run-action-v1 token captured from the flow run, mode 600,
#                                outside the repository; it is read and never printed
#   PROOF_SESSION_CORE_GRPC      tokens phase: session-core host:port, plaintext, on a proof stack only
#   PROOF_EXPIRED_TOKEN_FILE     optional: a token older than two minutes, for the expiry negative
#   PROOF_ZDR_OWNER_TOKEN, PROOF_ZDR_SPACE_REF, PROOF_ZDR_BINDING_REF
#                                zdr phase: a room in a zero-retention org
#   PROOF_SESSION_DB_MIN         the Session Core DB test floor (default 11)
#   PROOF_PHASES                 space-separated phase names (default: flow)
#
# The scripted model must propose space.thread.rename with PROOF_SCRIPTED_TITLE for the title request and
# space.thread.pin for the pin request. How the dev stack provides that model is open (WS18 open question 8).
#
# Never: reads a .env file; sets or changes a switch, a policy or a credential on any stack; prints, stores
#   or copies a token, key, password or DSN (a token reaches only a request body under TMP_DIR, removed on
#   exit); writes a ledger row; claims promotion. Evidence holds status codes, ids, kinds, digests, SQL rows
#   of ids and counts, and decoded claims with no signature segment.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
REPO_DIR="$(cd "$ROOT_DIR/../.." && pwd)"
APPS_DIR="$ROOT_DIR/.."
RUST_MANIFEST="$ROOT_DIR/rust/Cargo.toml"
PROTO_DIR="$ROOT_DIR/proto"
CAPABILITY_CORE_DIR="$ROOT_DIR/go/services/capability-core"
USER_CORE_DIR="$APPS_DIR/Control Plane/user-core"
GATEWAY_DIR="$APPS_DIR/Frontend Plane/verevonv3/apps/gateway"
GATEWAY_MANIFEST="$GATEWAY_DIR/Cargo.toml"
FRONTEND_DIR="$APPS_DIR/Frontend Plane/verevonv3"
CONVEX_DIR="$APPS_DIR/Application Plane/convex-core"
TMP_DIR="$(mktemp -d)"
MODE="${1:-full}"
EVIDENCE_DIR=""
CONTROL_CONTAINER="space-self-api-control-pg-${RANDOM}-$$"
CONTROL_PG_PASSWORD="$(openssl rand -hex 16)"
CONTROL_DSN=""

cleanup() {
  docker rm -f "$CONTROL_CONTAINER" >/dev/null 2>&1 || true
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

# record OUTCOME NAME: one summary line in the evidence directory, when there is one. Never a value.
record() {
  if [[ -n "$EVIDENCE_DIR" && -d "$EVIDENCE_DIR" ]]; then
    printf '%s %s\n' "$1" "$2" >>"$EVIDENCE_DIR/summary.txt"
  fi
}

# fail MESSAGE: the run is BLOCKED at this point. Nothing after it counts.
fail() {
  printf 'BLOCKED %s\n' "$1"
  record BLOCKED "$1"
  exit 2
}

# blocked_with_log NAME LOG: a step did not pass; show its log, with the disposable Postgres password redacted, and stop.
blocked_with_log() {
  printf 'BLOCKED %s\n' "$1"
  if [[ -f "$2" ]]; then
    sed -n '1,160p' "$2" | sed "s|${CONTROL_PG_PASSWORD}|[redacted]|g"
  fi
  record BLOCKED "$1"
  exit 2
}

# run_check NAME COMMAND...: passes only when the command exits 0.
run_check() {
  local name="$1"
  shift
  local log="$TMP_DIR/$name.log"
  printf 'PROOF %s\n' "$name"
  if "$@" >"$log" 2>&1; then
    printf 'PASS  %s\n' "$name"
    record PASS "$name"
  else
    blocked_with_log "$name" "$log"
  fi
}

# run_cargo NAME CARGO-ARGS...: passes only when cargo exits 0 and at least one test passed.
run_cargo() {
  local name="$1"
  shift
  local log="$TMP_DIR/$name.log"
  printf 'PROOF %s\n' "$name"
  if "$@" >"$log" 2>&1 && grep -Eq 'test result: ok\. [1-9][0-9]* passed' "$log"; then
    printf 'PASS  %s\n' "$name"
    record PASS "$name"
  else
    blocked_with_log "$name" "$log"
  fi
}

# run_go NAME DIR PATTERN PACKAGE...: passes only when at least one test passes and none fails or skips.
run_go() {
  local name="$1" dir="$2" pattern="$3"
  shift 3
  local log="$TMP_DIR/$name.log"
  printf 'PROOF %s\n' "$name"
  if (cd "$dir" && go test -count=1 -v -run "$pattern" "$@") >"$log" 2>&1 &&
    grep -q -- '--- PASS' "$log" &&
    ! grep -Eq -- '--- (FAIL|SKIP)' "$log"; then
    printf 'PASS  %s\n' "$name"
    record PASS "$name"
  else
    blocked_with_log "$name" "$log"
  fi
}

# run_tap NAME COMMAND...: a node test run; passes only when exit 0, at least one test passes and none
# fails, skips or is todo.
run_tap() {
  local name="$1"
  shift
  local log="$TMP_DIR/$name.log"
  printf 'PROOF %s\n' "$name"
  if "$@" >"$log" 2>&1 &&
    grep -Eq '^# pass [1-9][0-9]*' "$log" &&
    ! grep -Eq '^# (fail [1-9]|skipped [1-9]|todo [1-9])' "$log"; then
    printf 'PASS  %s\n' "$name"
    record PASS "$name"
  else
    blocked_with_log "$name" "$log"
  fi
}

# The proof checks goldens; it must never regenerate one, or it would compare a golden with itself.
no_golden_rewrite() {
  local switch
  for switch in VEREVON_UPDATE_GOLDEN MP_UPDATE_GOLDEN UPDATE_CONTRACT_GOLDEN; do
    if [[ -n "${!switch:-}" ]]; then
      printf '%s is set: a golden test would rewrite the golden it checks\n' "$switch"
      return 1
    fi
  done
}

# ---------------------------------------------------------------- layer 1: source

run_source_layers() {
  run_check source_no_golden_rewrite no_golden_rewrite
  # The registry sync test is owed by WS18-S1 (tests_owed); until it exists this step is BLOCKED.
  run_check source_registry_sync bash "$REPO_DIR/scripts/tests/space-self-api-registry-sync.test.sh"
  run_check source_capability_promotion_ledger bash "$ROOT_DIR/scripts/tests/capability-promotion-ledger-test.sh"

  run_go source_control_run_action "$USER_CORE_DIR" 'RunAction|ModelAction|OwnerEffect|SelfAPI' ./internal/spaces/ ./internal/http/
  run_go source_capability_core_v2 "$CAPABILITY_CORE_DIR" 'ModelActionView|OwnerAction|Availability|Authz|AgentActionAdmission' ./internal/api/ ./internal/authz/

  run_cargo source_session_core_self_api cargo test --manifest-path "$RUST_MANIFEST" -p session-core self_api
  local filter
  for filter in space_self_api client_tool_with_reserved_name_is_dropped plan_mode_and_subagent_refuse_registry_writes \
    execute_step_rpc_refuses_reserved_names per_action_tool; do
    run_cargo "source_execution_core_$filter" cargo test --manifest-path "$RUST_MANIFEST" -p execution-core "$filter"
  done
  run_cargo source_mp_contracts_space_self_api cargo test --manifest-path "$RUST_MANIFEST" -p mp-contracts space_self_api
  run_cargo source_model_gateway_self_api cargo test --manifest-path "$RUST_MANIFEST" -p model-gateway self_api
  run_cargo source_gateway_spaces cargo test --manifest-path "$GATEWAY_MANIFEST" --all-targets domains::spaces

  run_tap source_gateway_action_surface node --test "$GATEWAY_DIR/scripts/action-surface-contract.test.mjs"
  run_tap source_convex_space_agent_self_api bash -c "cd \"$CONVEX_DIR\" && npx tsx --test test/space-agent-self-api.test.cjs"
  run_check source_spa_vitest bash -c "cd \"$FRONTEND_DIR\" && pnpm exec vitest run src/shared/actions src/features/spaces src/shared/api"
  run_check source_spa_typecheck bash -c "cd \"$FRONTEND_DIR\" && pnpm typecheck"
  run_check source_spa_lint bash -c "cd \"$FRONTEND_DIR\" && pnpm lint"
}

# ---------------------------------------------------------------- layer 2: disposable Postgres

start_control_postgres() {
  docker create --pull=never --name "$CONTROL_CONTAINER" -P \
    -e POSTGRES_PASSWORD="$CONTROL_PG_PASSWORD" \
    -e POSTGRES_DB=user_core \
    postgres:16-alpine >/dev/null
  docker start "$CONTROL_CONTAINER" >/dev/null
  local port="" attempt
  for attempt in $(seq 1 120); do
    port="$(docker port "$CONTROL_CONTAINER" 5432/tcp 2>/dev/null | sed -n 's/.*:\([0-9][0-9]*\)$/\1/p' | head -1)"
    if [[ -n "$port" ]] && docker exec "$CONTROL_CONTAINER" pg_isready -U postgres -d user_core >/dev/null 2>&1; then
      CONTROL_DSN="postgres://postgres:${CONTROL_PG_PASSWORD}@127.0.0.1:${port}/user_core?sslmode=disable"
      return 0
    fi
    sleep 1
  done
  return 1
}

run_postgres_layers() {
  # The harness starts and removes its own container and runs the #[ignore] DB tests whose names contain self_api.
  run_check postgres_session_core_self_api bash "$ROOT_DIR/scripts/tests/session-core-db-tests.sh" "self_api=${PROOF_SESSION_DB_MIN:-11}"
  if ! start_control_postgres; then
    fail "the Control Postgres container did not become ready"
  fi
  export TEST_DATABASE_URL="$CONTROL_DSN"
  run_go postgres_control_run_action_matrix "$USER_CORE_DIR" 'TestRunActionDecisionMatrix|OwnerEffectReservation' ./internal/spaces/
  unset TEST_DATABASE_URL
}

# ---------------------------------------------------------------- layer 3: live phases

# plain_id NAME VALUE: refuses a value that is not a plain identifier before it reaches SQL or a body.
plain_id() {
  local pattern='^[A-Za-z0-9_.:-]+$'
  [[ "${2:-}" =~ $pattern ]] || fail "$1 is not a plain identifier"
}

live_inputs() {
  local name tool missing_input=0 title_pattern='^[A-Za-z0-9 ]+$'
  umask 077
  for name in PROOF_GATEWAY_URL PROOF_SESSION_DB_DSN PROOF_EVIDENCE_DIR PROOF_ORG_ID PROOF_SPACE_REF \
    PROOF_BINDING_REF PROOF_AGENT_SUBJECT_ID PROOF_OWNER_TOKEN PROOF_VIEWER_TOKEN PROOF_SCRIPTED_TITLE; do
    if [[ -z "${!name:-}" ]]; then
      printf 'missing live input (name only): %s\n' "$name"
      missing_input=1
    fi
  done
  [[ "$missing_input" -eq 0 ]] || fail "live inputs are not all set"
  for tool in curl node psql; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool is not on PATH"
  done
  mkdir -p "$PROOF_EVIDENCE_DIR"
  EVIDENCE_DIR="$(cd "$PROOF_EVIDENCE_DIR" && pwd)"
  case "$EVIDENCE_DIR" in
    "$REPO_DIR" | "$REPO_DIR"/*) fail "PROOF_EVIDENCE_DIR must be outside the repository" ;;
  esac
  [[ "$PROOF_SCRIPTED_TITLE" =~ $title_pattern ]] || fail "PROOF_SCRIPTED_TITLE must be letters, digits and spaces"
  plain_id PROOF_ORG_ID "$PROOF_ORG_ID"
  plain_id PROOF_SPACE_REF "$PROOF_SPACE_REF"
  plain_id PROOF_BINDING_REF "$PROOF_BINDING_REF"
  plain_id PROOF_AGENT_SUBJECT_ID "$PROOF_AGENT_SUBJECT_ID"
  # Every query below is a SELECT. The DSN is passed to psql and is never printed.
}

sql() { # QUERY: rows, pipe-separated, on stdout
  psql "$PROOF_SESSION_DB_DSN" -X -q -t -A -F '|' -v ON_ERROR_STOP=1 -c "$1"
}

# receipt_count THREAD: applied and noop receipts of the thread (refused calls write none, see depth).
receipt_count() {
  plain_id THREAD "$1"
  sql "SELECT count(*) FROM run_thread_action_receipts WHERE thread_id = '$1' AND outcome IN ('applied', 'noop')"
}

# http_call METHOD URL TOKEN BODY-FILE-OR-EMPTY OUT-FILE: prints the HTTP status only.
http_call() {
  local method="$1" url="$2" token="$3" body="$4" out="$5"
  if [[ -n "$body" ]]; then
    curl -sS -o "$out" -w '%{http_code}' -X "$method" -H "Authorization: Bearer ${token}" \
      -H 'Content-Type: application/json' --data-binary "@${body}" "$url" || true
  else
    curl -sS -o "$out" -w '%{http_code}' -X "$method" -H "Authorization: Bearer ${token}" "$url" || true
  fi
}

# toggle_self_api SPACE BINDING TOKEN true|false: PUT the binding switch; prints the status.
toggle_self_api() {
  local body="$TMP_DIR/toggle.json"
  printf '{"enabled":%s}' "$4" >"$body"
  http_call PUT "$PROOF_GATEWAY_URL/api/v1/spaces/$1/agents/$2/self-api" "$3" "$body" "$TMP_DIR/toggle.out"
}

# post_message SPACE THREAD-OR-EMPTY TOKEN CONTENT: a human post that mentions the proof agent; prints the
# status and leaves the response in post.out. An empty THREAD starts a thread; otherwise it is a reply.
post_message() {
  local url body="$TMP_DIR/mention.json"
  if [[ -n "$2" ]]; then
    url="$PROOF_GATEWAY_URL/api/v1/spaces/$1/threads/$2/messages"
  else
    url="$PROOF_GATEWAY_URL/api/v1/spaces/$1/threads"
  fi
  printf '{"content":"%s","client_send_id":"proof-%s-%s","mentions":{"people":[],"agents":[{"subject_id":"%s"}]}}' \
    "$4" "$(date +%s)" "$RANDOM" "$PROOF_AGENT_SUBJECT_ID" >"$body"
  http_call POST "$url" "$3" "$body" "$TMP_DIR/post.out"
}

# json_field FILE KEY: the first value of KEY anywhere in the JSON document (the response shape is not
# verified; see depth). Exits 1 when the key is absent.
json_field() {
  node -e '
    const fs = require("fs");
    const doc = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const want = process.argv[2];
    const find = (value) => {
      if (value && typeof value === "object") {
        if (!Array.isArray(value) && Object.prototype.hasOwnProperty.call(value, want)) {
          return value[want];
        }
        for (const item of Object.values(value)) {
          const hit = find(item);
          if (hit !== undefined) { return hit; }
        }
      }
      return undefined;
    };
    const hit = find(doc);
    if (hit === undefined || hit === null) { process.exit(1); }
    process.stdout.write(String(hit));
  ' "$1" "$2"
}

# wait_for_receipts THREAD MIN: polls until MIN applied or noop receipts exist, within PROOF_WAIT_SECONDS.
wait_for_receipts() {
  local thread="$1" want="$2" waited=0
  while ((waited < ${PROOF_WAIT_SECONDS:-90})); do
    if (($(receipt_count "$thread") >= want)); then
      return 0
    fi
    sleep 5
    waited=$((waited + 5))
  done
  return 1
}

# assert_thread_agent_owned THREAD: both overrides are the agent's and the title is the scripted one.
assert_thread_agent_owned() {
  local row title tsrc pinned psrc
  row="$(sql "SELECT presentation_title, title_source, (pinned_at IS NOT NULL), pinned_source FROM threads WHERE id = '$1'")"
  printf '%s\n' "$row" >"$EVIDENCE_DIR/sql-thread-flow.txt"
  IFS='|' read -r title tsrc pinned psrc <<<"$row"
  [[ "$title" == "$PROOF_SCRIPTED_TITLE" && "$tsrc" == agent && "$pinned" == t && "$psrc" == agent ]] ||
    fail "the thread is not agent-owned as expected (title, title_source, pinned_at, pinned_source)"
}

# assert_events_ids_only THREAD: the applied events exist and no event payload carries the title text.
assert_events_ids_only() {
  local applied leaked
  applied="$(sql "SELECT count(*) FROM events WHERE resource_ref = 'thread:$1' AND event_type = 'RUN_THREAD_ACTION_APPLIED'")"
  [[ "$applied" -ge 1 ]] || fail "no RUN_THREAD_ACTION_APPLIED event for the thread"
  leaked="$(sql "SELECT count(*) FROM events WHERE resource_ref = 'thread:$1' AND payload::text LIKE '%$PROOF_SCRIPTED_TITLE%'")"
  [[ "$leaked" == 0 ]] || fail "an event payload carries the title text"
}

record_phase() {
  printf '%s\n' "$1" >>"$EVIDENCE_DIR/phases.txt"
  printf 'PASS  phase %s\n' "$1"
}

phase_flow() {
  local code thread run_id before after
  printf 'PROOF phase flow\n'
  code="$(toggle_self_api "$PROOF_SPACE_REF" "$PROOF_BINDING_REF" "$PROOF_OWNER_TOKEN" true)"
  [[ "$code" == 200 ]] || fail "the owner's toggle on answered $code"
  code="$(post_message "$PROOF_SPACE_REF" "" "$PROOF_OWNER_TOKEN" 'Give this thread a title and pin it.')"
  [[ "$code" == 200 || "$code" == 201 ]] || fail "the owner's mention answered $code"
  thread="$(json_field "$TMP_DIR/post.out" thread_id)" || fail "the post response carries no thread_id (shape unverified)"
  plain_id THREAD "$thread"
  wait_for_receipts "$thread" 2 || fail "the rename and pin receipts did not appear within the wait"
  [[ "$(sql "SELECT count(*) FROM run_thread_action_receipts WHERE thread_id = '$thread' AND action_id IN ('space.thread.rename', 'space.thread.pin') AND outcome = 'applied'")" == 2 ]] ||
    fail "expected one applied rename and one applied pin"
  [[ "$(sql "SELECT count(*) FROM run_thread_action_receipts WHERE thread_id = '$thread' AND length(payload_digest) = 71")" == 2 ]] ||
    fail "a receipt does not carry a 71-character digest"
  sql "SELECT action_id, outcome, length(payload_digest), idempotency_key FROM run_thread_action_receipts WHERE thread_id = '$thread' ORDER BY action_id" \
    >"$EVIDENCE_DIR/sql-receipts-flow.txt"
  run_id="$(sql "SELECT run_id FROM run_thread_action_receipts WHERE thread_id = '$thread' ORDER BY action_id LIMIT 1")"
  plain_id RUN_ID "$run_id"
  printf 'thread_id=%s\nrun_id=%s\n' "$thread" "$run_id" >"$EVIDENCE_DIR/flow-ids.txt"
  assert_thread_agent_owned "$thread"
  assert_events_ids_only "$thread"

  # Negative: a human title is never overwritten by the agent.
  printf '{"title":"Human title"}' >"$TMP_DIR/title.json"
  code="$(http_call PATCH "$PROOF_GATEWAY_URL/api/v1/spaces/$PROOF_SPACE_REF/threads/$thread/presentation" \
    "$PROOF_OWNER_TOKEN" "$TMP_DIR/title.json" "$TMP_DIR/title.out")"
  [[ "$code" == 200 ]] || fail "the owner's title edit answered $code"
  [[ "$(sql "SELECT presentation_title || '|' || title_source FROM threads WHERE id = '$thread'")" == "Human title|owner" ]] ||
    fail "the human title edit did not take with title_source owner"
  before="$(sql "SELECT count(*) FROM run_thread_action_receipts WHERE thread_id = '$thread' AND action_id = 'space.thread.rename' AND outcome = 'applied'")"
  code="$(post_message "$PROOF_SPACE_REF" "$thread" "$PROOF_OWNER_TOKEN" 'Give this thread a new title.')"
  [[ "$code" == 200 || "$code" == 201 ]] || fail "the reply answered $code"
  sleep "${PROOF_WAIT_SECONDS:-90}"
  after="$(sql "SELECT count(*) FROM run_thread_action_receipts WHERE thread_id = '$thread' AND action_id = 'space.thread.rename' AND outcome = 'applied'")"
  [[ "$after" == "$before" ]] || fail "the agent renamed a thread a human had titled"
  [[ "$(sql "SELECT presentation_title || '|' || title_source FROM threads WHERE id = '$thread'")" == "Human title|owner" ]] ||
    fail "the human title changed"

  # Negative: with the switch off the same request offers no tool and writes no receipt.
  code="$(toggle_self_api "$PROOF_SPACE_REF" "$PROOF_BINDING_REF" "$PROOF_OWNER_TOKEN" false)"
  [[ "$code" == 200 ]] || fail "the owner's toggle off answered $code"
  before="$(receipt_count "$thread")"
  post_message "$PROOF_SPACE_REF" "$thread" "$PROOF_OWNER_TOKEN" 'Give this thread a title and pin it.' >/dev/null
  sleep "${PROOF_WAIT_SECONDS:-90}"
  [[ "$(receipt_count "$thread")" == "$before" ]] || fail "a receipt was written after the self-API switch went off"

  # Negative: a viewer cannot switch the self-API on.
  code="$(toggle_self_api "$PROOF_SPACE_REF" "$PROOF_BINDING_REF" "$PROOF_VIEWER_TOKEN" true)"
  [[ "$code" == 403 ]] || fail "a viewer's toggle answered $code, not 403"
  record_phase flow
}

# grpc_body TOKEN ACTION VALUE KEY: the ApplyRunThreadAction JSON on stdout. Values go through the environment.
grpc_body() {
  PROOF_ORG_ID="$PROOF_ORG_ID" PROOF_RUN_ID="$PROOF_RUN_ID" PROOF_TOKEN_VALUE="$1" PROOF_ACTION="$2" \
    PROOF_VALUE="$3" PROOF_KEY="$4" node -e '
    const action = process.env.PROOF_ACTION;
    const value = process.env.PROOF_VALUE;
    const body = {
      orgId: process.env.PROOF_ORG_ID,
      runId: process.env.PROOF_RUN_ID,
      actionId: action,
      idempotencyKey: process.env.PROOF_KEY,
      controlDecisionToken: process.env.PROOF_TOKEN_VALUE,
    };
    if (action === "space.thread.rename") { body.rename = { title: value }; }
    if (action === "space.thread.pin") { body.pin = { pinned: value === "true" }; }
    process.stdout.write(JSON.stringify(body));
  '
}

# grpc_apply TOKEN ACTION VALUE KEY: one ApplyRunThreadAction call to session-core. The exit status is the
# call's; the response is in grpc.out.
grpc_apply() {
  grpc_body "$1" "$2" "$3" "$4" >"$TMP_DIR/grpc.json"
  grpcurl -plaintext -import-path "$PROTO_DIR" -proto model_plane/v1/space_run.proto \
    -d @ "$PROOF_SESSION_CORE_GRPC" model_plane.v1.SpaceRunActions/ApplyRunThreadAction \
    <"$TMP_DIR/grpc.json" >"$TMP_DIR/grpc.out" 2>&1
}

# decode_claims TOKEN: the kid and the claims of a run-action-v1 token. The signature segment is never printed.
decode_claims() {
  PROOF_TOKEN_VALUE="$1" node -e '
    const parts = (process.env.PROOF_TOKEN_VALUE || "").split(".");
    if (parts.length !== 4 || parts[0] !== "run-action-v1") { process.exit(1); }
    const claims = JSON.parse(Buffer.from(parts[2], "base64url").toString("utf8"));
    process.stdout.write(JSON.stringify({ kid: parts[1], claims: claims }, null, 2) + "\n");
  '
}

phase_tokens() {
  local name token forged last alt key before expired
  printf 'PROOF phase tokens\n'
  for name in PROOF_DECISION_TOKEN_FILE PROOF_SESSION_CORE_GRPC PROOF_THREAD_ID PROOF_RUN_ID; do
    [[ -n "${!name:-}" ]] || fail "missing tokens input (name only): $name"
  done
  [[ -f "$PROOF_DECISION_TOKEN_FILE" ]] || fail "PROOF_DECISION_TOKEN_FILE is not a file"
  command -v grpcurl >/dev/null 2>&1 || fail "grpcurl is not on PATH"
  plain_id PROOF_THREAD_ID "$PROOF_THREAD_ID"
  plain_id PROOF_RUN_ID "$PROOF_RUN_ID"
  token="$(tr -d '\r\n' <"$PROOF_DECISION_TOKEN_FILE")"
  [[ "$token" == run-action-v1.* ]] || fail "the token file does not hold a run-action-v1 token"
  if ! decode_claims "$token" >"$EVIDENCE_DIR/decision-claims.json"; then
    fail "the token does not decode as run-action-v1 claims"
  fi
  key="$(sql "SELECT idempotency_key FROM run_thread_action_receipts WHERE thread_id = '$PROOF_THREAD_ID' AND action_id = 'space.thread.rename' AND outcome = 'applied'")"
  plain_id idempotency_key "$key"
  before="$(receipt_count "$PROOF_THREAD_ID")"

  # Replay inside two minutes: the same token, key and arguments answer REPLAYED and write nothing.
  grpc_apply "$token" space.thread.rename "$PROOF_SCRIPTED_TITLE" "$key" || true
  grep -q 'RUN_ACTION_OUTCOME_REPLAYED' "$TMP_DIR/grpc.out" ||
    fail "the replay did not answer REPLAYED (a token older than two minutes is refused, not replayed)"

  # Forged signature: the last character changed.
  last="${token: -1}"
  if [[ "$last" == A ]]; then alt=B; else alt=A; fi
  forged="${token%?}$alt"
  if grpc_apply "$forged" space.thread.rename "$PROOF_SCRIPTED_TITLE" "$key"; then
    fail "a forged decision token was applied"
  fi
  # Changed arguments under the same token and key: a different digest.
  if grpc_apply "$token" space.thread.rename "A changed title" "$key"; then
    fail "changed arguments under the decision token were applied"
  fi
  # Another action's arguments under a rename token.
  if grpc_apply "$token" space.thread.pin true "$key"; then
    fail "a rename token applied another action"
  fi
  if [[ -n "${PROOF_EXPIRED_TOKEN_FILE:-}" ]]; then
    [[ -f "$PROOF_EXPIRED_TOKEN_FILE" ]] || fail "PROOF_EXPIRED_TOKEN_FILE is not a file"
    expired="$(tr -d '\r\n' <"$PROOF_EXPIRED_TOKEN_FILE")"
    if grpc_apply "$expired" space.thread.rename "$PROOF_SCRIPTED_TITLE" "$key"; then
      fail "an expired decision token was applied"
    fi
  fi
  [[ "$(receipt_count "$PROOF_THREAD_ID")" == "$before" ]] || fail "a refused call wrote a receipt"
  record_phase tokens
}

phase_allowlist_empty() {
  local before code
  printf 'PROOF phase allowlist-empty\n'
  # The operator emptied CONTROL_SPACE_SELF_API_ACTIONS on user-core and recreated it (runbook step). The
  # proof cannot read that setting; the evidence line for it is the operator's, by name only.
  plain_id PROOF_THREAD_ID "${PROOF_THREAD_ID:-}"
  code="$(toggle_self_api "$PROOF_SPACE_REF" "$PROOF_BINDING_REF" "$PROOF_OWNER_TOKEN" true)"
  [[ "$code" == 200 ]] || fail "the owner's toggle on answered $code"
  before="$(receipt_count "$PROOF_THREAD_ID")"
  code="$(post_message "$PROOF_SPACE_REF" "$PROOF_THREAD_ID" "$PROOF_OWNER_TOKEN" 'Give this thread a title and pin it.')"
  [[ "$code" == 200 || "$code" == 201 ]] || fail "the reply answered $code"
  sleep "${PROOF_WAIT_SECONDS:-90}"
  [[ "$(receipt_count "$PROOF_THREAD_ID")" == "$before" ]] || fail "a receipt was written with the allowlist empty"
  record_phase allowlist-empty
}

phase_zdr() {
  local name code receipts agent_owned
  printf 'PROOF phase zdr\n'
  for name in PROOF_ZDR_OWNER_TOKEN PROOF_ZDR_SPACE_REF PROOF_ZDR_BINDING_REF; do
    [[ -n "${!name:-}" ]] || fail "missing zdr input (name only): $name"
  done
  plain_id PROOF_ZDR_SPACE_REF "$PROOF_ZDR_SPACE_REF"
  plain_id PROOF_ZDR_BINDING_REF "$PROOF_ZDR_BINDING_REF"
  code="$(toggle_self_api "$PROOF_ZDR_SPACE_REF" "$PROOF_ZDR_BINDING_REF" "$PROOF_ZDR_OWNER_TOKEN" true)"
  printf 'zdr toggle status %s\n' "$code" >>"$EVIDENCE_DIR/zdr-phase.txt"
  code="$(post_message "$PROOF_ZDR_SPACE_REF" "" "$PROOF_ZDR_OWNER_TOKEN" 'Give this thread a title and pin it.')"
  printf 'zdr post status %s\n' "$code" >>"$EVIDENCE_DIR/zdr-phase.txt"
  sleep "${PROOF_WAIT_SECONDS:-90}"
  receipts="$(sql "SELECT count(*) FROM run_thread_action_receipts r JOIN threads t ON t.id = r.thread_id WHERE t.space_id = '$PROOF_ZDR_SPACE_REF'")"
  agent_owned="$(sql "SELECT count(*) FROM threads WHERE space_id = '$PROOF_ZDR_SPACE_REF' AND (title_source = 'agent' OR pinned_source = 'agent')")"
  [[ "$receipts" == 0 && "$agent_owned" == 0 ]] || fail "a zero-retention room received a Space action"
  record_phase zdr
}

verdict() {
  local phase recorded
  recorded="$(cat "$EVIDENCE_DIR/phases.txt" 2>/dev/null || true)"
  for phase in flow tokens allowlist-empty zdr; do
    if grep -qx "$phase" <<<"$recorded"; then
      printf 'recorded phase: %s\n' "$phase"
    else
      printf 'not recorded yet: %s\n' "$phase"
    fi
  done
}

# ---------------------------------------------------------------- main

case "$MODE" in
  source | full) ;;
  *) fail "unknown mode: $MODE (use source or full)" ;;
esac

if [[ "$MODE" == full ]]; then
  live_inputs # fail fast on missing live inputs, before the long source layers run
fi

run_source_layers
run_postgres_layers

if [[ "$MODE" == source ]]; then
  printf 'WS18-S10 source and postgres layers passed. This is not the WS18-S10 proof: no live phase ran.\n'
  exit 0
fi

for phase in ${PROOF_PHASES:-flow}; do
  case "$phase" in
    flow) phase_flow ;;
    tokens) phase_tokens ;;
    allowlist-empty) phase_allowlist_empty ;;
    zdr) phase_zdr ;;
    *) fail "unknown phase: $phase" ;;
  esac
done
verdict
printf 'WS18-S10 layers and phases above are this run only. The full proof needs the source and postgres layers in one run\n'
printf 'and all four phases recorded in one evidence directory. Not claimed here: promotion. Capability rows stay\n'
printf 'source_only until a candidate and live evidence are recorded (docs/runbooks/space-self-api-evidence-template.md).\n'
