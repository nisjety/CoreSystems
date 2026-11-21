#!/usr/bin/env bash
# Contract test for scripts/acceptance-live-metrics.sh (F-f01-07). Every scrape, evidence file, policy
# and stub release tool below is an invented fixture under a scratch directory: nothing reads a running
# plane, a real policy file or a real rehearsal record. It proves what each live source measures, that an
# unreadable source is never a pass, that a proposed row never passes, and that an approved row fails
# over its target.
set -euo pipefail
export LC_ALL=C

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT="$ROOT_DIR/scripts/acceptance-live-metrics.sh"
REAL_THRESHOLDS="$ROOT_DIR/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv"
TMP="$(mktemp -d "${TMPDIR:-/tmp}/acceptance-live-metrics-test.XXXXXX")"
trap 'rm -rf "$TMP"' EXIT

TODAY=2026-11-20
checks=0
failures=0
pass() { checks=$((checks + 1)); }
fail() {
  printf 'not ok - %s\n' "$1" >&2
  failures=$((failures + 1))
  checks=$((checks + 1))
}

# ---- fixtures ---------------------------------------------------------------------------------------

# Gateway, histogram form: 1000 requests, 100 of them 5xx. The POST route holds 100 observations with
# cumulative buckets 50/90/100, the health route 10 under 0.1 s. Merged: 60/100/110/110, so the 0.95 rank
# is 104.5, which falls in (0.5, 1] at 0.5 + 0.5 * (104.5 - 100) / (110 - 100) = 0.725 s = 725.0 ms.
GW_HISTOGRAM="$TMP/gateway-histogram.metrics"
cat >"$GW_HISTOGRAM" <<'METRICS'
# HELP gateway_requests_total requests by outcome
# TYPE gateway_requests_total counter
gateway_requests_total{method="POST",route="/v1/ai/chat",status="200"} 800
gateway_requests_total{method="POST",route="/v1/ai/chat",status="503"} 100
gateway_requests_total{method="GET",route="/healthz",status="200"} 100
# TYPE gateway_request_duration_seconds histogram
gateway_request_duration_seconds_bucket{method="POST",route="/v1/ai/chat",le="0.1"} 50
gateway_request_duration_seconds_bucket{method="POST",route="/v1/ai/chat",le="0.5"} 90
gateway_request_duration_seconds_bucket{method="POST",route="/v1/ai/chat",le="1"} 100
gateway_request_duration_seconds_bucket{method="POST",route="/v1/ai/chat",le="+Inf"} 100
gateway_request_duration_seconds_sum{method="POST",route="/v1/ai/chat"} 40
gateway_request_duration_seconds_count{method="POST",route="/v1/ai/chat"} 100
gateway_request_duration_seconds_bucket{method="GET",route="/healthz",le="0.1"} 10
gateway_request_duration_seconds_bucket{method="GET",route="/healthz",le="0.5"} 10
gateway_request_duration_seconds_bucket{method="GET",route="/healthz",le="1"} 10
gateway_request_duration_seconds_bucket{method="GET",route="/healthz",le="+Inf"} 10
gateway_request_duration_seconds_sum{method="GET",route="/healthz"} 0.5
gateway_request_duration_seconds_count{method="GET",route="/healthz"} 10
METRICS

# Gateway, summary form (what the gateway exports today: no buckets are configured). The 0.5 quantile
# is ignored; the largest 0.95 quantile is 1.2 s, so the p95 is 1200.0 ms.
GW_SUMMARY="$TMP/gateway-summary.metrics"
cat >"$GW_SUMMARY" <<'METRICS'
# HELP gateway_requests_total requests by outcome
# TYPE gateway_requests_total counter
gateway_requests_total{method="POST",route="/v1/ai/chat",status="200"} 800
gateway_requests_total{method="POST",route="/v1/ai/chat",status="503"} 100
gateway_requests_total{method="GET",route="/healthz",status="200"} 100
# TYPE gateway_request_duration_seconds summary
gateway_request_duration_seconds{method="POST",route="/v1/ai/chat",quantile="0.5"} 0.2
gateway_request_duration_seconds{method="POST",route="/v1/ai/chat",quantile="0.95"} 1.2
gateway_request_duration_seconds{method="GET",route="/healthz",quantile="0.95"} 0.004
gateway_request_duration_seconds_sum{method="POST",route="/v1/ai/chat"} 150
gateway_request_duration_seconds_count{method="POST",route="/v1/ai/chat"} 1000
METRICS

# The gateway with no metrics recorder: the placeholder body has no series at all. A zero here would be a
# false pass, so it must read as unreadable.
GW_PLACEHOLDER="$TMP/gateway-placeholder.metrics"
printf '# HELP model_gateway_requests_total\n' >"$GW_PLACEHOLDER"

# A gateway whose rolling summary windows are empty: every quantile renders 0, which is no data.
GW_SUMMARY_IDLE="$TMP/gateway-summary-idle.metrics"
cat >"$GW_SUMMARY_IDLE" <<'METRICS'
# TYPE gateway_request_duration_seconds summary
gateway_request_duration_seconds{method="POST",route="/v1/ai/chat",quantile="0.95"} 0
gateway_request_duration_seconds{method="GET",route="/healthz",quantile="0.95"} 0
gateway_request_duration_seconds_sum{method="POST",route="/v1/ai/chat"} 150
gateway_request_duration_seconds_count{method="POST",route="/v1/ai/chat"} 1000
METRICS

SESSION_OK="$TMP/session.metrics"
cat >"$SESSION_OK" <<'METRICS'
# TYPE mp_session_audit_outbox_pending gauge
mp_session_audit_outbox_pending 3
# TYPE mp_session_audit_outbox_terminal gauge
mp_session_audit_outbox_terminal 0
# TYPE mp_session_audit_outbox_oldest_pending_seconds gauge
mp_session_audit_outbox_oldest_pending_seconds 12.5
METRICS

EVIDENCE_PASS="$TMP/evidence-pass"
mkdir -p "$EVIDENCE_PASS"
printf '# Rollback rehearsal: result PASSED\nresult: PASSED\n' >"$EVIDENCE_PASS/rollback-rehearsal-2026-11-17.txt"

# The newest rehearsal failed, so the older PASSED file must not be read as the current state.
EVIDENCE_FAILED="$TMP/evidence-failed"
mkdir -p "$EVIDENCE_FAILED"
printf '# Rollback rehearsal: result PASSED\n' >"$EVIDENCE_FAILED/rollback-rehearsal-2026-11-10.txt"
printf '# Rollback rehearsal: result FAILED\nresult: FAILED\n' >"$EVIDENCE_FAILED/rollback-rehearsal-2026-11-19.txt"

EVIDENCE_EMPTY="$TMP/evidence-empty"
mkdir -p "$EVIDENCE_EMPTY"

# A leap-year gap: 2000-02-28 to 2000-03-01 is two days.
EVIDENCE_LEAP="$TMP/evidence-leap"
mkdir -p "$EVIDENCE_LEAP"
printf '# Rollback rehearsal: result PASSED\n' >"$EVIDENCE_LEAP/rollback-rehearsal-2000-02-28.txt"

# Stub release tools: the report shape check-zdr-attestation prints, with and without a claim.
STUB_DAYS="$TMP/release-tool-days.sh"
cat >"$STUB_DAYS" <<'STUB'
#!/usr/bin/env bash
printf 'AZURE_OPENAI: attestation valid, review by 2027-01-19 (60 days left)\n'
printf 'AZURE_ANTHROPIC: attestation valid, review by 2027-02-10 (82 days left)\n'
STUB
STUB_NONE="$TMP/release-tool-none.sh"
cat >"$STUB_NONE" <<'STUB'
#!/usr/bin/env bash
printf 'AZURE_OPENAI: no ZDR claim\n'
printf 'AZURE_ANTHROPIC: no ZDR claim\n'
STUB

# Threshold fixtures. The row format is the real one: id, unit, target, operator, owner, source, stop rule, status.
row() {  # <id> <unit> <target> <operator> <status>
  printf '%s\t%s\t%s\t%s\tfixture owner\tfixture source\tfixture stop rule\t%s\n' "$1" "$2" "$3" "$4" "$5"
}
tsv() {  # <file> <row>...
  local file="$1"
  shift
  {
    printf '# fixture thresholds\n'
    printf '%s\n' "$@"
  } >"$file"
}

TSV_LIVE_PROPOSED="$TMP/live-proposed.tsv"
tsv "$TSV_LIVE_PROPOSED" \
  "$(row gateway_error_ratio_per_mille per_mille 150 '<=' proposed)" \
  "$(row gateway_p95_latency_ms milliseconds unset '<=' proposed)" \
  "$(row audit_outbox_terminal_rows rows 0 '==' proposed)" \
  "$(row audit_delivery_lag_seconds seconds unset '<=' proposed)" \
  "$(row zdr_attestation_days_to_review days unset '>=' proposed)" \
  "$(row rollback_rehearsal_age_days days 30 '<=' proposed)"

TSV_ALL_APPROVED="$TMP/all-approved.tsv"
tsv "$TSV_ALL_APPROVED" \
  "$(row gateway_error_ratio_per_mille per_mille 150 '<=' approved)" \
  "$(row gateway_p95_latency_ms milliseconds 1500 '<=' approved)" \
  "$(row audit_outbox_terminal_rows rows 0 '==' approved)" \
  "$(row audit_delivery_lag_seconds seconds 60 '<=' approved)" \
  "$(row zdr_attestation_days_to_review days 30 '>=' approved)" \
  "$(row rollback_rehearsal_age_days days 30 '<=' approved)"

# ---- runner -----------------------------------------------------------------------------------------

# The environment every run starts from: fixture scrapes, a fixture evidence directory, a fixture
# policy path and a stub release tool. A test overrides only what it means to change.
base_env() {
  RUN_ENV=(
    "MP_ACCEPTANCE_TODAY=$TODAY"
    "MP_GATEWAY_METRICS_URL=file://$GW_SUMMARY"
    "MP_SESSION_METRICS_URL=file://$SESSION_OK"
    "CORESYSTEM_ROLLBACK_EVIDENCE_DIR=$EVIDENCE_PASS"
    "ACCEPTANCE_THRESHOLDS_FILE=$TSV_LIVE_PROPOSED"
    "MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE=$TMP/policy-placeholder.txt"
    "MODEL_PLANE_RELEASE_ARTIFACT_SCRIPT=$STUB_DAYS"
  )
}

# run <args of the script> : sets OUT (stdout and stderr) and RC, with RUN_ENV applied.
run() {
  local status=0
  OUT="$(env "${RUN_ENV[@]}" bash "$SCRIPT" "$@" 2>&1)" || status=$?
  RC=$status
}

# The override helper: replace one KEY=value entry of RUN_ENV.
set_env() {  # <KEY> <value>
  local i
  for i in "${!RUN_ENV[@]}"; do
    case "${RUN_ENV[$i]}" in
      "$1="*) RUN_ENV[i]="$1=$2"; return 0 ;;
    esac
  done
  RUN_ENV+=("$1=$2")
}

expect_line() {  # <description> <exact line>
  if grep -Fxq -- "$2" <<<"$OUT"; then pass; else fail "$1: no line '$2' in: $(tr '\n' '|' <<<"$OUT")"; fi
}
expect_no_line_starting() {  # <description> <prefix>
  if grep -q -- "^$2" <<<"$OUT"; then fail "$1: unexpected line starting '$2' in: $(tr '\n' '|' <<<"$OUT")"; else pass; fi
}
expect_rc() {  # <description> <code>
  if [[ "$RC" -eq "$2" ]]; then pass; else fail "$1: exit $RC, expected $2 (output: $(tr '\n' '|' <<<"$OUT"))"; fi
}

# ---- --measure: what each live source reads --------------------------------------------------------

base_env
run --measure
expect_rc "measure reads every source" 0
expect_line "error ratio is the 5xx share per mille" "gateway_error_ratio_per_mille 100.000 per_mille"
expect_line "summary p95 is the largest 0.95 quantile" "gateway_p95_latency_ms 1200.0 milliseconds"
expect_line "terminal rows are the gauge" "audit_outbox_terminal_rows 0 rows"
expect_line "delivery lag is the oldest pending age" "audit_delivery_lag_seconds 12.5 seconds"
expect_line "ZDR days is the fewest days left" "zdr_attestation_days_to_review 60 days"
expect_line "rehearsal age is days since the newest PASSED file" "rollback_rehearsal_age_days 3 days"
if [[ "$(grep -c '' <<<"$OUT")" -eq 6 ]]; then pass; else fail "measure prints one line per live row: $(tr '\n' '|' <<<"$OUT")"; fi

set_env MP_GATEWAY_METRICS_URL "file://$GW_HISTOGRAM"
run --measure
expect_line "histogram p95 is interpolated over every route" "gateway_p95_latency_ms 725.0 milliseconds"
expect_line "histogram error ratio is unchanged" "gateway_error_ratio_per_mille 100.000 per_mille"

set_env MP_GATEWAY_METRICS_URL "file://$GW_SUMMARY_IDLE"
run --measure
expect_line "an idle summary has no p95, and its zero quantiles are not a pass" "gateway_p95_latency_ms unreadable milliseconds"
expect_line "a gateway with no request counters has no error ratio" "gateway_error_ratio_per_mille unreadable per_mille"

set_env MP_GATEWAY_METRICS_URL "file://$GW_PLACEHOLDER"
set_env MP_SESSION_METRICS_URL "file://$TMP/absent-session.metrics"
set_env MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE ""
run --measure
expect_rc "an unreadable source fails the measure" 1
expect_line "a placeholder gateway is unreadable, not zero" "gateway_error_ratio_per_mille unreadable per_mille"
expect_line "a placeholder gateway has no p95" "gateway_p95_latency_ms unreadable milliseconds"
expect_line "a missing session scrape is unreadable" "audit_outbox_terminal_rows unreadable rows"
expect_line "a missing session scrape has no lag" "audit_delivery_lag_seconds unreadable seconds"
expect_line "no policy path means no ZDR figure" "zdr_attestation_days_to_review unreadable days"
expect_line "the rehearsal source is still read" "rollback_rehearsal_age_days 3 days"

base_env
set_env CORESYSTEM_ROLLBACK_EVIDENCE_DIR "$EVIDENCE_FAILED"
run --measure
expect_rc "a failed newest rehearsal fails the measure" 1
expect_line "the newest rehearsal decides, not the older PASSED file" "rollback_rehearsal_age_days failed days"

base_env
set_env CORESYSTEM_ROLLBACK_EVIDENCE_DIR "$EVIDENCE_EMPTY"
run --measure
expect_line "no rehearsal file is unreadable" "rollback_rehearsal_age_days unreadable days"

base_env
set_env CORESYSTEM_ROLLBACK_EVIDENCE_DIR "$EVIDENCE_LEAP"
set_env MP_ACCEPTANCE_TODAY 2000-03-01
run --measure
expect_line "the day count crosses a leap day" "rollback_rehearsal_age_days 2 days"

base_env
set_env MODEL_PLANE_RELEASE_ARTIFACT_SCRIPT "$STUB_NONE"
run --measure
expect_line "a policy with no ZDR claim is unreadable, never a pass" "zdr_attestation_days_to_review unreadable days"

# ---- --check: proposed rows never pass ---------------------------------------------------------------

base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_LIVE_PROPOSED"
run --check
expect_rc "today's state is blocked, not passed" 3
expect_line "a proposed row with a passing live value is blocked" "BLOCKED proposed audit_outbox_terminal_rows"
expect_no_line_starting "no proposed row passes" "PASS "
expect_no_line_starting "no proposed row fails as approved" "FAIL "
expect_line "an unset proposed target prints its baseline" "BASELINE gateway_p95_latency_ms 1200.0"
expect_line "a baseline for the ZDR figure is the live value" "BASELINE zdr_attestation_days_to_review 60"
expect_line "the summary counts the blocked rows" "acceptance live metrics: 6 rows, 0 approved, 0 passing, 0 failing, 6 not approved"

set_env MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE ""
run --check
expect_rc "an unreadable baseline still blocks" 3
expect_line "a baseline with no data prints unreadable" "BASELINE zdr_attestation_days_to_review unreadable"
expect_no_line_starting "an unreadable baseline passes nothing" "PASS "

set_env ACCEPTANCE_THRESHOLDS_FILE "$REAL_THRESHOLDS"
run --check
expect_rc "the real thresholds file is blocked while every row is proposed" 3
expect_no_line_starting "the real file passes nothing" "PASS "
if [[ "$(grep -c '^BLOCKED proposed ' <<<"$OUT")" -eq 17 ]]; then pass; else fail "every real row is blocked: $(tr '\n' '|' <<<"$OUT")"; fi
if [[ "$(grep -c '^BASELINE ' <<<"$OUT")" -eq 7 ]]; then pass; else fail "the seven unset rows print a baseline"; fi

# ---- --check: approved rows are judged against their targets ---------------------------------------

TSV_ONE_APPROVED="$TMP/one-approved.tsv"
tsv "$TSV_ONE_APPROVED" "$(row audit_outbox_terminal_rows rows 0 '==' approved)" \
  "$(row audit_delivery_lag_seconds seconds unset '<=' proposed)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_ONE_APPROVED"
run --check
expect_line "an approved row that meets its target passes" "PASS audit_outbox_terminal_rows 0 rows"
expect_rc "one passing approved row does not unblock other proposed rows" 3

TSV_LAG_APPROVED="$TMP/lag-approved.tsv"
tsv "$TSV_LAG_APPROVED" "$(row audit_delivery_lag_seconds seconds 5 '<=' approved)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_LAG_APPROVED"
run --check
expect_line "an approved row over its ceiling fails" "FAIL audit_delivery_lag_seconds 12.5 seconds"
expect_rc "an approved failure exits 1" 1

TSV_RATIO_APPROVED="$TMP/ratio-approved.tsv"
tsv "$TSV_RATIO_APPROVED" "$(row gateway_error_ratio_per_mille per_mille 150 '<=' approved)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_RATIO_APPROVED"
set_env MP_GATEWAY_METRICS_URL "file://$GW_PLACEHOLDER"
run --check
expect_line "an approved row whose source cannot be read is never a pass" "UNREADABLE gateway_error_ratio_per_mille"
expect_rc "an unreadable approved source exits 1" 1

TSV_NO_SOURCE="$TMP/no-source.tsv"
tsv "$TSV_NO_SOURCE" "$(row decision_ttl_seconds seconds 120 '<=' approved)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_NO_SOURCE"
run --check
expect_line "an approved row with no live source is not a pass" "NO-SOURCE decision_ttl_seconds"
expect_rc "an approved row with no source exits 1" 1

TSV_REHEARSAL_APPROVED="$TMP/rehearsal-approved.tsv"
tsv "$TSV_REHEARSAL_APPROVED" "$(row rollback_rehearsal_age_days days 30 '<=' approved)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_REHEARSAL_APPROVED"
set_env CORESYSTEM_ROLLBACK_EVIDENCE_DIR "$EVIDENCE_FAILED"
run --check
expect_line "an approved rehearsal that did not pass fails" "FAIL rollback_rehearsal_age_days failed days"
expect_rc "a failed approved rehearsal exits 1" 1

TSV_UNSET_APPROVED="$TMP/unset-approved.tsv"
tsv "$TSV_UNSET_APPROVED" "$(row gateway_p95_latency_ms milliseconds unset '<=' approved)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_UNSET_APPROVED"
run --check
expect_line "an approved row with an unset target is invalid" "INVALID gateway_p95_latency_ms an unset target can only be proposed"
expect_rc "an invalid approved row exits 1" 1

TSV_REVOKED="$TMP/revoked.tsv"
tsv "$TSV_REVOKED" "$(row audit_outbox_terminal_rows rows 0 '==' revoked)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_REVOKED"
run --check
expect_line "a revoked row is blocked, never passed" "BLOCKED revoked audit_outbox_terminal_rows"
expect_rc "a revoked row keeps the run blocked" 3

base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_ALL_APPROVED"
run --check
expect_rc "every row approved and met exits 0" 0
expect_line "the approved summary p95 meets its ceiling" "PASS gateway_p95_latency_ms 1200.0 milliseconds"
expect_line "the approved ratio meets its ceiling" "PASS gateway_error_ratio_per_mille 100.000 per_mille"
expect_line "the approved ZDR days meet their floor" "PASS zdr_attestation_days_to_review 60 days"
expect_line "the approved rehearsal meets its ceiling" "PASS rollback_rehearsal_age_days 3 days"
expect_line "the summary of an all-approved run" "acceptance live metrics: 6 rows, 6 approved, 6 passing, 0 failing, 0 not approved"

TSV_BAD_CONTRACT="$TMP/bad-contract.tsv"
tsv "$TSV_BAD_CONTRACT" \
  "$(row audit_outbox_terminal_rows rows 0 '~=' approved)" \
  "$(row audit_delivery_lag_seconds seconds soon '<=' approved)" \
  "$(row rollback_rehearsal_age_days days 30 '<=' waiting)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_BAD_CONTRACT"
run --check
expect_line "an operator the contract does not allow is invalid" "INVALID audit_outbox_terminal_rows operator ~= is not one the contract allows"
expect_line "a target that is not an integer is invalid" "INVALID audit_delivery_lag_seconds target soon is not an integer"
expect_line "an unknown status is invalid" "INVALID rollback_rehearsal_age_days unknown status waiting"
expect_rc "invalid rows exit 1" 1

TSV_NO_LIVE="$TMP/no-live.tsv"
tsv "$TSV_NO_LIVE" "$(row decision_ttl_seconds seconds 120 '<=' proposed)"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_NO_LIVE"
run --measure
expect_rc "a file with no live row cannot be measured as a pass" 1

# ---- malformed input and usage ------------------------------------------------------------------------

TSV_BROKEN="$TMP/broken.tsv"
printf '# broken\nrollback_rehearsal_age_days\tdays\t30\t<=\towner\tsource\tstop\n' >"$TSV_BROKEN"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_BROKEN"
run --check
expect_rc "a row without eight columns is a broken file" 2

TSV_EMPTY="$TMP/empty.tsv"
printf '# no rows\n' >"$TSV_EMPTY"
base_env
set_env ACCEPTANCE_THRESHOLDS_FILE "$TSV_EMPTY"
run --check
expect_rc "a file with no rows is a broken file" 2

base_env
run
expect_rc "no mode is a usage error" 2
run --bogus
expect_rc "an unknown mode is a usage error" 2
set_env MP_ACCEPTANCE_TODAY 2026-13-40
run --measure
expect_rc "an impossible date is refused" 2

if [[ "$failures" -ne 0 ]]; then
  printf 'acceptance live metrics test: %s of %s checks failed\n' "$failures" "$checks" >&2
  exit 1
fi
printf 'acceptance live metrics test: %s checks passed\n' "$checks"
