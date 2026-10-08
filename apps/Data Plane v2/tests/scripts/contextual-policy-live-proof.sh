#!/usr/bin/env bash
# Live proof of the organisation processing policy, the coverage endpoint and the release gates
# (CR-T11). It runs against the LIVE stack, after
#   * the migration 20261003130000_context_provenance is applied (CR-T6),
#   * the data-quality image carrying services/data-quality-go/internal/{policy,gates} and
#   * the embedding-engine image carrying services/embedding-engine-rs/src/policy.rs
# are deployed through the launcher stage copies (never a hand-built `docker compose`).
#
# SPEND. Steps `preflight`, `off`, `verify-off`, `coverage` and `gates` make NO provider call: the
# whole point of `off` is that the synthetic ingest of the throwaway organisation makes zero
# language-model calls, measured twice (the engine's own batch log, and the inference-core request
# counter if CR-T3 is deployed). Step `lift` writes the policy back to the default; what happens to
# the units after that belongs to the reconciler (CR-T9a) and step `verify-lifted` makes
# EXACTLY ONE paid gpt-4o-mini call per chunk of the synthetic document (three) through it:
# state that number in the proof report. The ingest itself is the orchestrator's, through
# documents-api, as in the CR-T7 and CR-T8 proofs; this script never ingests.
#
# Everything this script does is read-only EXCEPT the two policy writes (`off` and `lift`), which
# go through the audited admin API of data-quality, for the ONE throwaway organisation named in
# CR_LIVE_ORG_ID, with an operator token the orchestrator mints for that organisation.
#
# Nothing here prints a secret. The operator token is read from a FILE (CR_LIVE_OPERATOR_TOKEN_FILE)
# and handed to curl on stdin, never on a command line and never echoed; no environment of any
# container is printed except non-secret settings by name. Do not add `set -x`.
#
# The steps, in order (each needs CR_LIVE_ORG_ID):
#
#   CR_LIVE_STEP=preflight CR_LIVE_ORG_ID=<throwaway org> bash tests/scripts/contextual-policy-live-proof.sh
#       FREE, read-only. P1 the policy table, the audit table and v_context_coverage exist;
#       P2 the data-quality service answers /readyz and serves the three new routes (an
#       unauthenticated call answers 401, not 404); P3 the embedding engine booted with
#       contextual retrieval enabled and logged no schema refusal; P4 the organisation has no
#       policy row yet; prints the counts of rows in org_processing_policy and the audit rows.
#
#   CR_LIVE_STEP=off CR_LIVE_ORG_ID=<org> CR_LIVE_OPERATOR_TOKEN_FILE=<file> bash ...
#       WRITES one policy row. O1 PUT off with a reason answers 200, action `set`, effective off;
#       O2 GET reads it back identically; O3 the audit log holds one new row for the organisation
#       (action org_processing_policy.set, the principal, a request id); O4 NEGATIVE: the same
#       token naming ANOTHER organisation in the path is refused 403 tenant_mismatch and writes
#       nothing; O5 NEGATIVE (when CR_LIVE_USER_TOKEN_FILE is a user token of the organisation):
#       refused 403 operator_only, and the policy is unchanged; O6 an invalid body (an off
#       without a reason) is refused 400 and the policy is unchanged; O7 records the
#       inference-core request counter for the engine before the ingest (the state file).
#       Then the orchestrator ingests the synthetic document for the organisation, waits for
#       `indexed`, and records the document id.
#
#   CR_LIVE_STEP=verify-off CR_LIVE_ORG_ID=<org> CR_LIVE_DOCUMENT_ID=<doc> bash ...
#       Read-only. V1 every unit of the document is embedding_status done, context_status
#       skipped_policy, chunk_context NULL, no error class, embedded_context_hash NULL (embedded
#       raw); V2 v_context_coverage says skipped_policy equals the number of units; V3 the engine
#       logged the batch with provider_calls=0 and context_outcomes naming skipped_policy;
#       V4 the engine never asked Auth Core for a bearer for the organisation (no mint log line
#       for it) when the log carries that evidence; V5 the inference-core request counter for
#       the engine is unchanged since step `off` (zero language-model calls at the provider
#       boundary), when CR-T3's counters exist.
#
#   CR_LIVE_STEP=lift CR_LIVE_ORG_ID=<org> CR_LIVE_OPERATOR_TOKEN_FILE=<file> bash ...
#       WRITES. L1 PUT with every switch inherit answers 200, action `reset`, and the row is
#       gone (count 0); L2 the audit log holds the `reset` row whose before is the off policy;
#       L3 GET reads the default (source default, effective on).
#
#   CR_LIVE_STEP=verify-lifted CR_LIVE_ORG_ID=<org> CR_LIVE_DOCUMENT_ID=<doc> bash ...
#       Read-only, waits up to LIFT_WAIT_SECONDS (default 300) for the reconciler of CR-T9a to
#       work the units: D1 the units leave skipped_policy (to pending, then ready); D2 every unit
#       ends ready with a 64-hex context_input_hash and embedded_context_hash equal to the sha256
#       of its stored context (coherent); D3 v_context_coverage says ready equals the number of
#       units and vector_incoherent = 0. Paid: one call per chunk (see SPEND).
#
#   CR_LIVE_STEP=coverage CR_LIVE_ORG_ID=<the LIVE org> CR_LIVE_OPERATOR_TOKEN_FILE=<file> \
#       EXPECT_READY=<n> EXPECT_LEGACY=<n> EXPECT_PENDING=<n> bash ...
#       Read-only. C1 GET /v1/quality/context-coverage answers 200 with exactly the expected
#       counts (the live organisation of 2026-10-06 reads ready 0, legacy 206, pending 1 before
#       the legacy verification pass, and ready 207, legacy 0, pending 0 after it); C2 the counts
#       agree with a direct v_context_coverage read; C3 evaluation.status is `failed` while
#       legacy or pending units remain and `passed` once EXPECT_READY equals the eligible count.
#
#   CR_LIVE_STEP=gates CR_LIVE_ORG_ID=<org> CR_LIVE_OPERATOR_TOKEN_FILE=<file> \
#       EXPECT_COVERAGE_GATE=<passed|failed|skipped_policy> EXPECT_PARITY_GATE=<passed|failed|unavailable> bash ...
#       Read-only. G1 GET /v1/quality/gates carries both gates with the expected status; G2 an
#       `unavailable` gate has passed=false and the report is a 412; before CR-T9a the parity gate
#       is `unavailable` (EXPECT_PARITY_GATE=unavailable), after it `passed`.
#
# Variables
#   CR_LIVE_STEP                preflight | off | verify-off | lift | verify-lifted | coverage | gates
#   CR_LIVE_ORG_ID              required
#   CR_LIVE_DOCUMENT_ID         required for verify-off and verify-lifted
#   CR_LIVE_OPERATOR_TOKEN_FILE a file holding an operator service token for THAT organisation
#   CR_LIVE_USER_TOKEN_FILE     optional: a user token of the organisation, for the negative O5
#   CR_LIVE_STATE_DIR           default ${TMPDIR:-/tmp}; the state file of O7/V5 lives there
#   EXPECTED_CHUNKS             default 3
#   LIFT_WAIT_SECONDS           default 300
#   POSTGRES_CONTAINER          default data-plane-v2-postgres-1   (database dataplane, role dataplane)
#   QUALITY_CONTAINER           default data-plane-v2-data-quality-1
#   ENGINE_CONTAINER            default data-plane-v2-embedding-engine-1
#   INFERENCE_METRICS_URL       default http://127.0.0.1:18082/metrics
set -euo pipefail
set +x

STEP="${CR_LIVE_STEP:?set CR_LIVE_STEP to preflight, off, verify-off, lift, verify-lifted, coverage or gates}"
ORG="${CR_LIVE_ORG_ID:?set CR_LIVE_ORG_ID to a throwaway organization id}"
DOC="${CR_LIVE_DOCUMENT_ID:-}"
OPERATOR_TOKEN_FILE="${CR_LIVE_OPERATOR_TOKEN_FILE:-}"
USER_TOKEN_FILE="${CR_LIVE_USER_TOKEN_FILE:-}"
STATE_DIR="${CR_LIVE_STATE_DIR:-${TMPDIR:-/tmp}}"
CHUNKS="${EXPECTED_CHUNKS:-3}"
LIFT_WAIT="${LIFT_WAIT_SECONDS:-300}"
PG="${POSTGRES_CONTAINER:-data-plane-v2-postgres-1}"
DQ="${QUALITY_CONTAINER:-data-plane-v2-data-quality-1}"
ENGINE="${ENGINE_CONTAINER:-data-plane-v2-embedding-engine-1}"
METRICS="${INFERENCE_METRICS_URL:-http://127.0.0.1:18082/metrics}"
API="http://127.0.0.1:8013"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
pass() { printf 'PASS: %s\n' "$*"; }
note() { printf '      %s\n' "$*"; }

[[ $ORG =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || fail "CR_LIVE_ORG_ID is not a valid organization id"
[[ -z $DOC || $DOC =~ ^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$ ]] || fail "CR_LIVE_DOCUMENT_ID is not a valid document id"
for container in "$PG" "$DQ" "$ENGINE"; do
  docker inspect "$container" >/dev/null 2>&1 || fail "container $container is not running"
done

# One read-only psql call: the SQL on stdin, the ids as variables (:'org'), never spliced into SQL.
psql_q() {
  printf '%s\n' "$1" | docker exec -i "$PG" psql -U dataplane -d dataplane -X -q -tA -F '|' \
    -v ON_ERROR_STOP=1 -v org="$ORG" -v doc="$DOC"
}

need_doc() { [[ -n $DOC ]] || fail "CR_LIVE_DOCUMENT_ID is required for the $STEP step"; }
need_token() {
  [[ -n $OPERATOR_TOKEN_FILE && -r $OPERATOR_TOKEN_FILE ]] || fail "CR_LIVE_OPERATOR_TOKEN_FILE must name a readable file holding an operator token for the organisation"
}

# api <token-file> <method> <path> [json body]: prints the body, then the HTTP status on the last line.
# The token is written to a mode-600 header file INSIDE the container from stdin, handed to curl with
# -H @file, and removed: it is never an argument, never printed, never on a host command line.
api() {
  local tokenfile=$1 method=$2 path=$3 body=${4:-}
  printf 'Authorization: Bearer %s\n' "$(tr -d '\r\n' <"$tokenfile")" |
    docker exec -i "$DQ" sh -c '
      umask 077; h=$(mktemp); cat >"$h"
      if [ -n "$3" ]; then
        curl -s -o - -w "\n%{http_code}" -X "$0" -H @"$h" -H "Accept: application/json" -H "Content-Type: application/json" --data-binary "$3" "$1$2"
      else
        curl -s -o - -w "\n%{http_code}" -X "$0" -H @"$h" -H "Accept: application/json" "$1$2"
      fi
      rc=$?; rm -f "$h"; exit $rc' "$method" "$API" "$path" "$body"
}

status_of() { printf '%s\n' "$1" | tail -n1; }
body_of() { printf '%s\n' "$1" | sed '$d'; }
# json_get <json> <dotted.path>: a value from the JSON, or empty. Needs node on the host.
json_get() {
  printf '%s' "$1" | node -e '
    let s = "";
    process.stdin.on("data", (c) => (s += c)).on("end", () => {
      try {
        let v = JSON.parse(s);
        for (const k of process.argv[1].split(".")) v = v == null ? undefined : v[k];
        process.stdout.write(v === undefined || v === null ? "" : typeof v === "object" ? JSON.stringify(v) : String(v));
      } catch { process.stdout.write(""); }
    });' "$2"
}

policy_path="/v1/admin/orgs/$ORG/processing-policy"
state_file="$STATE_DIR/cr-t11-policy-proof-$ORG.state"
off_body='{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit","reason":"CR-T11 live proof: throwaway evaluation organisation"}'
default_body='{"contextual_retrieval":"inherit","graph_extraction":"inherit","budget_mode":"inherit"}'

metric_total() { # sum of inference_requests_total samples for the embedding-engine service, or "none"
  local text
  text=$(curl -s --max-time 5 "$METRICS" 2>/dev/null || true)
  if ! grep -q '^inference_requests_total' <<<"$text"; then printf 'none'; return; fi
  grep '^inference_requests_total' <<<"$text" | grep 'service_id="embedding-engine"' |
    awk '{ s += $NF } END { printf "%d", s + 0 }'
}

case "$STEP" in
  preflight)
    [[ $(psql_q "SELECT to_regclass('public.org_processing_policy') IS NOT NULL AND to_regclass('public.v_context_coverage') IS NOT NULL AND to_regclass('public.admin_audit_log') IS NOT NULL;") == t ]] \
      || fail "P1: org_processing_policy, v_context_coverage or admin_audit_log is missing: apply migration 20261003130000_context_provenance first"
    pass "P1 the policy table, the coverage view and the audit table exist"
    docker exec "$DQ" curl -sf "$API/readyz" >/dev/null || fail "P2: data-quality /readyz does not answer"
    for route in "GET /v1/quality/context-coverage" "GET $policy_path" "PUT $policy_path"; do
      method=${route%% *}; path=${route#* }
      code=$(docker exec "$DQ" curl -s -o /dev/null -w '%{http_code}' -X "$method" "$API$path")
      [[ $code == 401 ]] || fail "P2: an unauthenticated $method $path answered $code, want 401 (404 means the image does not carry CR-T11)"
    done
    pass "P2 data-quality is ready and serves the three routes behind authentication"
    logs=$(docker logs "$ENGINE" 2>&1 | tail -n 400)
    grep -qiE 'contextual retrieval ENABLED' <<<"$logs" || fail "P3: the embedding engine did not log that contextual retrieval is enabled"
    ! grep -qiE 'has no column|apply migration' <<<"$logs" || fail "P3: the embedding engine logged a schema refusal"
    pass "P3 the embedding engine booted with contextual retrieval enabled and no schema refusal"
    [[ $(psql_q "SELECT count(*) FROM org_processing_policy WHERE org_id = :'org';") == 0 ]] || fail "P4: the organisation already has a policy row"
    pass "P4 the organisation has no policy row yet"
    note "org_processing_policy rows: $(psql_q "SELECT count(*) FROM org_processing_policy;"), policy audit rows: $(psql_q "SELECT count(*) FROM admin_audit_log WHERE target_kind = 'org_processing_policy';")"
    pass "preflight"
    ;;

  off)
    need_token
    audit_before=$(psql_q "SELECT count(*) FROM admin_audit_log WHERE org_id = :'org' AND target_kind = 'org_processing_policy';")
    out=$(api "$OPERATOR_TOKEN_FILE" PUT "$policy_path" "$off_body")
    [[ $(status_of "$out") == 200 ]] || fail "O1: PUT off answered $(status_of "$out"): $(body_of "$out" | head -c 300)"
    b=$(body_of "$out")
    [[ $(json_get "$b" action) == set && $(json_get "$b" effective.contextual_retrieval) == off ]] || fail "O1: the response is not a set of off: $(printf '%s' "$b" | head -c 300)"
    pass "O1 PUT off answered 200, action set, effective off"
    got=$(api "$OPERATOR_TOKEN_FILE" GET "$policy_path")
    [[ $(status_of "$got") == 200 && $(json_get "$(body_of "$got")" policy.contextual_retrieval) == off && $(json_get "$(body_of "$got")" policy.source) == policy ]] \
      || fail "O2: GET does not read back the off policy"
    pass "O2 GET reads back the off policy"
    audit_after=$(psql_q "SELECT count(*) FROM admin_audit_log WHERE org_id = :'org' AND target_kind = 'org_processing_policy';")
    (( audit_after == audit_before + 1 )) || fail "O3: audit rows went $audit_before -> $audit_after, want exactly one more"
    [[ $(psql_q "SELECT action || '|' || (actor <> '') || '|' || (request_id IS NOT NULL) FROM admin_audit_log WHERE org_id = :'org' AND target_kind = 'org_processing_policy' ORDER BY audit_id DESC LIMIT 1;") == "org_processing_policy.set|t|t" ]] \
      || fail "O3: the audit row is not a set by a named principal with a request id"
    pass "O3 one audit row: org_processing_policy.set, by a named principal, with a request id"
    other="${ORG}-other"
    other_out=$(api "$OPERATOR_TOKEN_FILE" PUT "/v1/admin/orgs/$other/processing-policy" "$off_body")
    [[ $(status_of "$other_out") == 403 && $(json_get "$(body_of "$other_out")" code) == tenant_mismatch ]] \
      || fail "O4: naming another organisation answered $(status_of "$other_out"), want 403 tenant_mismatch"
    [[ $(psql_q "SELECT count(*) FROM org_processing_policy WHERE org_id = '$other';") == 0 ]] || fail "O4: a cross-organisation write left a row"
    pass "O4 a token of this organisation cannot write another's policy (403 tenant_mismatch, nothing written)"
    if [[ -n $USER_TOKEN_FILE && -r $USER_TOKEN_FILE ]]; then
      user_out=$(api "$USER_TOKEN_FILE" PUT "$policy_path" "$default_body")
      [[ $(status_of "$user_out") == 403 && $(json_get "$(body_of "$user_out")" code) == operator_only ]] \
        || fail "O5: a user token answered $(status_of "$user_out"), want 403 operator_only"
      [[ $(psql_q "SELECT contextual_retrieval FROM org_processing_policy WHERE org_id = :'org';") == off ]] || fail "O5: the user's refused PUT changed the policy"
      pass "O5 an organisation's own user token cannot change the policy (403 operator_only)"
    else
      note "O5 skipped: CR_LIVE_USER_TOKEN_FILE is not set (the negative for a user principal was not exercised live)"
    fi
    bad_out=$(api "$OPERATOR_TOKEN_FILE" PUT "$policy_path" '{"contextual_retrieval":"off","graph_extraction":"inherit","budget_mode":"inherit"}')
    [[ $(status_of "$bad_out") == 400 ]] || fail "O6: an off without a reason answered $(status_of "$bad_out"), want 400"
    [[ $(psql_q "SELECT contextual_retrieval FROM org_processing_policy WHERE org_id = :'org';") == off ]] || fail "O6: the refused PUT changed the policy"
    pass "O6 an off without a reason is refused 400 and changes nothing"
    mkdir -p "$STATE_DIR"
    printf 'metric_before=%s\noff_at=%s\n' "$(metric_total)" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >"$state_file"
    note "O7 recorded the inference-core request counter for the engine: $(sed -n 's/^metric_before=//p' "$state_file") (none = the counters are not deployed)"
    note "The policy reaches the engine within 30 seconds (its cache): wait 35 s, then ingest the synthetic document for $ORG and record its id."
    pass "off"
    ;;

  verify-off)
    need_doc
    [[ -r $state_file ]] || fail "run the off step first: $state_file is missing"
    units=$(psql_q "SELECT count(*) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc';")
    [[ $units == "$CHUNKS" ]] || fail "the document has $units units, EXPECTED_CHUNKS is $CHUNKS"
    bad=$(psql_q "SELECT count(*) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc' AND NOT (embedding_status = 'done' AND context_status = 'skipped_policy' AND chunk_context IS NULL AND context_error_class IS NULL AND embedded_context_hash IS NULL);")
    [[ $bad == 0 ]] || fail "V1: $bad unit(s) are not done / skipped_policy / without a context / embedded raw"
    pass "V1 every unit is done, skipped_policy, without a context and embedded raw"
    [[ $(psql_q "SELECT skipped_policy || '|' || ready || '|' || eligible FROM v_context_coverage WHERE org_id = :'org';") == "$CHUNKS|0|$CHUNKS" ]] \
      || fail "V2: v_context_coverage does not say skipped_policy=$CHUNKS ready=0 eligible=$CHUNKS"
    pass "V2 the coverage view counts the marker: skipped_policy=$CHUNKS"
    off_at=$(sed -n 's/^off_at=//p' "$state_file")
    line=$(docker logs --since "$off_at" "$ENGINE" 2>&1 | grep -E 'batch embedded' | grep -F "$DOC" | tail -n1 || true)
    if [[ -n $line ]]; then
      grep -qE "provider_calls[=\": ]+0([^0-9]|$)" <<<"$line" || fail "V3: the engine's batch line does not say provider_calls=0: $(printf '%s' "$line" | head -c 400)"
      grep -q 'skipped_policy' <<<"$line" || fail "V3: the engine's batch line does not name skipped_policy: $(printf '%s' "$line" | head -c 400)"
      pass "V3 the engine logged the batch with provider_calls=0 and context_outcomes naming skipped_policy"
    else
      note "V3 no batch line for the document in the engine log since $off_at (the log may have rolled); V1 and V2 above hold, but the engine's own count could not be shown"
    fi
    if docker logs --since "$off_at" "$ENGINE" 2>&1 | grep -F "$ORG" | grep -qiE 'bearer|mint|token'; then
      fail "V4: the engine log mentions a bearer or token for the organisation while it is off"
    fi
    pass "V4 no bearer was requested for the organisation (no mint line names it)"
    before=$(sed -n 's/^metric_before=//p' "$state_file")
    after=$(metric_total)
    if [[ $before == none || $after == none ]]; then
      note "V5 skipped: the inference-core request counter is not deployed (CR-T3), so zero provider calls rests on V1-V4"
    else
      [[ $before == "$after" ]] || fail "V5: inference_requests_total for embedding-engine moved $before -> $after during the ingest"
      pass "V5 inference-core counted no language-model call for the engine across the ingest ($before -> $after)"
    fi
    pass "verify-off"
    ;;

  lift)
    need_token
    out=$(api "$OPERATOR_TOKEN_FILE" PUT "$policy_path" "$default_body")
    [[ $(status_of "$out") == 200 && $(json_get "$(body_of "$out")" action) == reset ]] || fail "L1: PUT inherit answered $(status_of "$out"), want 200 with action reset"
    [[ $(psql_q "SELECT count(*) FROM org_processing_policy WHERE org_id = :'org';") == 0 ]] || fail "L1: the policy row is still there"
    pass "L1 PUT with every switch inherit reset the policy and removed the row"
    [[ $(psql_q "SELECT action || '|' || (payload->'before'->>'contextual_retrieval') FROM admin_audit_log WHERE org_id = :'org' AND target_kind = 'org_processing_policy' ORDER BY audit_id DESC LIMIT 1;") == "org_processing_policy.reset|off" ]] \
      || fail "L2: the last audit row is not a reset whose before is the off policy"
    pass "L2 the audit log holds the reset, with the off policy as its before"
    got=$(api "$OPERATOR_TOKEN_FILE" GET "$policy_path")
    [[ $(json_get "$(body_of "$got")" policy.source) == default && $(json_get "$(body_of "$got")" effective.contextual_retrieval) == on ]] || fail "L3: GET does not read the default"
    pass "L3 GET reads the default: source default, effective on"
    note "The engine applies it within 30 seconds. Whether the skipped_policy units are worked again is the reconciler's (CR-T9a): run verify-lifted."
    pass "lift"
    ;;

  verify-lifted)
    need_doc
    deadline=$(( $(date +%s) + LIFT_WAIT ))
    while :; do
      left=$(psql_q "SELECT count(*) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc' AND context_status IS DISTINCT FROM 'ready';")
      [[ $left == 0 ]] && break
      (( $(date +%s) < deadline )) || fail "D1/D2: $left unit(s) are still not ready after ${LIFT_WAIT}s (statuses: $(psql_q "SELECT string_agg(DISTINCT coalesce(context_status,'null'), ',') FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc';")); is CONTEXT_RECONCILER_ENABLED=1?"
      sleep 5
    done
    pass "D1 the units left skipped_policy and are ready"
    bad=$(psql_q "SELECT count(*) FROM knowledge_units WHERE org_id = :'org' AND document_id = :'doc' AND NOT (context_input_hash ~ '^[0-9a-f]{64}\$' AND embedded_context_hash = encode(sha256(convert_to(chunk_context, 'UTF8')), 'hex'));")
    [[ $bad == 0 ]] || fail "D2: $bad unit(s) have no 64-hex input hash or a vector not built from the stored context"
    pass "D2 every unit has its provenance and a vector built from its stored context"
    [[ $(psql_q "SELECT ready || '|' || vector_incoherent FROM v_context_coverage WHERE org_id = :'org';") == "$CHUNKS|0" ]] || fail "D3: v_context_coverage does not say ready=$CHUNKS vector_incoherent=0"
    pass "D3 the coverage view says ready=$CHUNKS and vector_incoherent=0"
    note "Paid calls made by the reconciler for this step: $CHUNKS (one per chunk); state it in the report."
    pass "verify-lifted"
    ;;

  coverage)
    need_token
    : "${EXPECT_READY:?set EXPECT_READY}" "${EXPECT_LEGACY:?set EXPECT_LEGACY}" "${EXPECT_PENDING:?set EXPECT_PENDING}"
    out=$(api "$OPERATOR_TOKEN_FILE" GET "/v1/quality/context-coverage")
    [[ $(status_of "$out") == 200 ]] || fail "C1: GET context-coverage answered $(status_of "$out")"
    b=$(body_of "$out")
    for pair in "ready:$EXPECT_READY" "legacy:$EXPECT_LEGACY" "pending:$EXPECT_PENDING"; do
      field=${pair%%:*}; want=${pair#*:}
      [[ $(json_get "$b" "counts.$field") == "$want" ]] || fail "C1: counts.$field is $(json_get "$b" "counts.$field"), want $want"
    done
    pass "C1 the endpoint returns ready=$EXPECT_READY legacy=$EXPECT_LEGACY pending=$EXPECT_PENDING"
    [[ $(psql_q "SELECT ready || '|' || legacy || '|' || pending || '|' || eligible FROM v_context_coverage WHERE org_id = :'org';") == "$EXPECT_READY|$EXPECT_LEGACY|$EXPECT_PENDING|$(json_get "$b" counts.eligible)" ]] \
      || fail "C2: the endpoint and a direct read of v_context_coverage disagree"
    pass "C2 the endpoint agrees with a direct read of v_context_coverage"
    status=$(json_get "$b" evaluation.status)
    if (( EXPECT_LEGACY + EXPECT_PENDING > 0 )); then
      [[ $status == failed ]] || fail "C3: evaluation.status is $status while legacy/pending units remain, want failed"
    else
      [[ $status == passed ]] || fail "C3: evaluation.status is $status with every unit ready, want passed"
    fi
    pass "C3 evaluation.status is $status ($(json_get "$b" evaluation.message | head -c 200))"
    pass "coverage"
    ;;

  gates)
    need_token
    : "${EXPECT_COVERAGE_GATE:?set EXPECT_COVERAGE_GATE}" "${EXPECT_PARITY_GATE:?set EXPECT_PARITY_GATE}"
    out=$(api "$OPERATOR_TOKEN_FILE" GET "/v1/quality/gates")
    code=$(status_of "$out"); b=$(body_of "$out")
    [[ $code == 200 || $code == 412 ]] || fail "G1: GET gates answered $code"
    gate_status() { printf '%s' "$b" | node -e '
      let s = ""; process.stdin.on("data", (c) => (s += c)).on("end", () => {
        const g = (JSON.parse(s).gates || []).find((x) => x.gate === process.argv[1]);
        process.stdout.write(g ? `${g.status || ""}|${g.passed}` : "missing");
      });' "$1"; }
    cov=$(gate_status contextual_coverage_ok); par=$(gate_status sparse_index_in_sync)
    [[ ${cov%%|*} == "$EXPECT_COVERAGE_GATE" ]] || fail "G1: contextual_coverage_ok is $cov, want $EXPECT_COVERAGE_GATE"
    [[ ${par%%|*} == "$EXPECT_PARITY_GATE" ]] || fail "G1: sparse_index_in_sync is $par, want $EXPECT_PARITY_GATE"
    pass "G1 contextual_coverage_ok=$EXPECT_COVERAGE_GATE sparse_index_in_sync=$EXPECT_PARITY_GATE"
    if [[ $EXPECT_PARITY_GATE == unavailable ]]; then
      [[ ${par##*|} == false && $code == 412 ]] || fail "G2: an unavailable gate must not pass and must fail the report (got passed=${par##*|}, HTTP $code)"
      pass "G2 the unavailable gate is not a pass and the report is a 412"
    fi
    pass "gates"
    ;;

  *) fail "unknown CR_LIVE_STEP '$STEP'" ;;
esac
