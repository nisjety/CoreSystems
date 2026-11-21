#!/usr/bin/env bash
# Acceptance live metrics (F-f01-07): measure the live signals that the acceptance thresholds name,
# and judge each approved row against its target. Read only: it scrapes the planes' own /metrics
# endpoints, reads the rollback-rehearsal evidence directory, and runs the release tool's read-only
# attestation report. It prints aggregate numbers and closed words only: no tenant, thread, run or
# content identifier, no credential, and no policy file content.
#
#   scripts/acceptance-live-metrics.sh --measure   one line per threshold row that has a live source:
#                                                  '<id> <value> <unit>'
#   scripts/acceptance-live-metrics.sh --check     every threshold row, judged by its status and target
#
# Live sources, by threshold id:
#   gateway_error_ratio_per_mille   model-gateway gateway_requests_total: the 5xx share per mille of all
#                                   requests, over every route and method
#   gateway_p95_latency_ms          model-gateway gateway_request_duration_seconds, in milliseconds. When
#                                   the gateway exports histogram buckets, the p95 is interpolated over
#                                   every route and method (the Prometheus histogram_quantile rule). The
#                                   gateway exports a summary today (it configures no buckets), so the
#                                   value is the largest per-route 0.95 quantile. No route's p95 can be
#                                   exceeded by more than 5% of its own requests, so this value is an
#                                   upper bound on the gateway-wide p95.
#   audit_outbox_terminal_rows      session-core mp_session_audit_outbox_terminal
#   audit_delivery_lag_seconds      session-core mp_session_audit_outbox_oldest_pending_seconds
#   zdr_attestation_days_to_review  the fewest days left in the release tool's check-zdr-attestation
#                                   report for MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE
#   rollback_rehearsal_age_days     days since the newest docs/compliance/evidence/rollback-rehearsal-<date>.txt.
#                                   Like the conformance gate, it reads the first line: 'failed' unless it says
#                                   'result PASSED'
#
# A source that cannot be read, or has no data yet, prints 'unreadable'. A rehearsal whose first line does
# not say PASSED prints 'failed'. Neither is ever a pass.
#
# --check lines:
#   PASS <id> <value> <unit>        approved, and the live value meets the approved target
#   FAIL <id> <value> <unit>        approved, and the live value does not meet it (or the rehearsal failed)
#   UNREADABLE <id>                 approved, and its live source cannot be read
#   NO-SOURCE <id>                  approved, and nothing measures it live
#   INVALID <id> <reason>           a row the threshold contract does not allow
#   BLOCKED <status> <id>           proposed or revoked: never a pass
#   BASELINE <id> <value>           target unset (proposed rows only): the live value, for the 7-day baseline
#
# Exit codes for --check: 0 every row is approved and passes. 1 an approved row fails, is unreadable, has no
# live source or is invalid. 3 nothing failed, but at least one row is not approved, so a conformance run stays
# blocked (today every row is proposed). 2 usage, or a missing or malformed thresholds file.
# --measure exits 0 only when every live source gave a value (no 'unreadable' and no 'failed'), and 1 otherwise
# (2 for the same file errors).
#
# Environment (defaults in brackets):
#   MP_GATEWAY_METRICS_URL                    [http://127.0.0.1:8080/metrics]   model-gateway HTTP port
#   MP_SESSION_METRICS_URL                    [http://127.0.0.1:18081/metrics]  session-core HTTP port (8081 in the container)
#   ACCEPTANCE_THRESHOLDS_FILE                [docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv]
#   MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE    the signed runtime public policy, for the ZDR attestation
#   MODEL_PLANE_RELEASE_ARTIFACT_SCRIPT       [scripts/release-artifact.sh]
#   CORESYSTEM_ROLLBACK_EVIDENCE_DIR          [docs/compliance/evidence]
#   MP_ACCEPTANCE_TODAY                       [today, UTC] YYYY-MM-DD, for a reproducible run
# A file:// URL is read from disk; the contract test uses it for fixture scrapes. Scrapes send no credentials.
set -euo pipefail
export LC_ALL=C

usage() {
  echo "usage: scripts/acceptance-live-metrics.sh --measure | --check" >&2
}

case "${1:-}" in
  --measure | --check)
    [[ $# -eq 1 ]] || {
      usage
      exit 2
    }
    ;;
  *)
    usage
    exit 2
    ;;
esac

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
THRESHOLDS="${ACCEPTANCE_THRESHOLDS_FILE:-$ROOT_DIR/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv}"
GATEWAY_METRICS_URL="${MP_GATEWAY_METRICS_URL:-http://127.0.0.1:8080/metrics}"
SESSION_METRICS_URL="${MP_SESSION_METRICS_URL:-http://127.0.0.1:18081/metrics}"
RELEASE_TOOL="${MODEL_PLANE_RELEASE_ARTIFACT_SCRIPT:-$ROOT_DIR/scripts/release-artifact.sh}"
REHEARSAL_EVIDENCE_DIR="${CORESYSTEM_ROLLBACK_EVIDENCE_DIR:-$ROOT_DIR/../../docs/compliance/evidence}"
TODAY="${MP_ACCEPTANCE_TODAY:-$(date -u '+%Y-%m-%d')}"
LIVE_IDS=" gateway_error_ratio_per_mille gateway_p95_latency_ms audit_outbox_terminal_rows audit_delivery_lag_seconds zdr_attestation_days_to_review rollback_rehearsal_age_days "

# The Prometheus text helpers. series() splits one sample line into SNAME, SLABELS and SVALUE; label()
# reads one label of the current sample. Every awk program below starts with these functions.
read -r -d '' AWK_SERIES <<'AWK' || true
function series(line,   rest, i, parts) {
  SNAME = ""; SLABELS = ""; SVALUE = ""
  if (!match(line, /^[a-zA-Z_:][a-zA-Z0-9_:]*/)) return 0
  SNAME = substr(line, 1, RLENGTH)
  rest = substr(line, RLENGTH + 1)
  if (substr(rest, 1, 1) == "{") {
    i = index(rest, "}")
    if (i == 0) return 0
    SLABELS = substr(rest, 2, i - 2)
    rest = substr(rest, i + 1)
  }
  sub(/^[ \t]+/, "", rest)
  split(rest, parts, /[ \t]+/)
  if (parts[1] == "") return 0
  SVALUE = parts[1]
  return 1
}
function label(name,   text, i, rest, j) {
  text = "," SLABELS
  i = index(text, "," name "=\"")
  if (i == 0) return ""
  rest = substr(text, i + length(name) + 3)
  j = index(rest, "\"")
  return substr(rest, 1, j - 1)
}
AWK

# Requests by status: the 5xx share, per mille of every request the gateway counted.
read -r -d '' AWK_RATIO <<'AWK' || true
BEGIN { total = 0; errors = 0; seen = 0 }
/^gateway_requests_total/ {
  if (!series($0) || SNAME != "gateway_requests_total") next
  seen++
  total += SVALUE + 0
  if (label("status") ~ /^5[0-9][0-9]$/) errors += SVALUE + 0
}
END {
  if (seen == 0 || total <= 0) { print "unreadable"; exit 0 }
  printf "%.3f\n", 1000 * errors / total
}
AWK

# Request duration: histogram buckets summed over every route and method, then the 0.95 quantile by
# linear interpolation (histogram_quantile). Without buckets, the largest per-route 0.95 summary quantile.
read -r -d '' AWK_P95 <<'AWK' || true
BEGIN { buckets = 0; quantiles = 0; maxq = 0 }
/^gateway_request_duration_seconds/ {
  if (!series($0)) next
  if (SNAME == "gateway_request_duration_seconds_bucket") {
    le = label("le")
    if (le == "") next
    cum_by_le[le] += SVALUE + 0
    buckets++
  } else if (SNAME == "gateway_request_duration_seconds" && label("quantile") == "0.95" && SVALUE ~ /^[0-9.eE+-]+$/ && SVALUE + 0 > 0) {
    # A summary whose rolling window holds no sample renders 0 for every quantile. A real request
    # duration is never 0, so a 0 is no data, and it is left out (an idle gateway is unreadable).
    quantiles++
    if (SVALUE + 0 > maxq) maxq = SVALUE + 0
  }
}
END {
  if (buckets > 0) {
    n = 0
    for (k in cum_by_le) {
      n++
      bound[n] = (k == "+Inf") ? 1e300 : k + 0
      cum[n] = cum_by_le[k]
    }
    for (i = 1; i <= n; i++)
      for (j = i + 1; j <= n; j++)
        if (bound[j] < bound[i]) {
          t = bound[i]; bound[i] = bound[j]; bound[j] = t
          t = cum[i]; cum[i] = cum[j]; cum[j] = t
        }
    if (cum[n] <= 0) { print "unreadable"; exit 0 }
    rank = 0.95 * cum[n]
    prev_bound = 0; prev_cum = 0
    for (i = 1; i <= n; i++) {
      if (cum[i] >= rank) {
        if (bound[i] >= 1e299) s = prev_bound
        else if (cum[i] == prev_cum) s = bound[i]
        else s = prev_bound + (bound[i] - prev_bound) * (rank - prev_cum) / (cum[i] - prev_cum)
        printf "%.1f\n", s * 1000
        exit 0
      }
      prev_bound = bound[i]; prev_cum = cum[i]
    }
    print "unreadable"; exit 0
  }
  if (quantiles > 0) { printf "%.1f\n", maxq * 1000; exit 0 }
  print "unreadable"
}
AWK

# One unlabelled gauge: exactly one sample, a plain number. The variables want and kind come from -v.
read -r -d '' AWK_GAUGE <<'AWK' || true
series($0) && SNAME == want && SLABELS == "" { found++; value = SVALUE }
END {
  if (found != 1 || value !~ /^[0-9]+(\.[0-9]+)?([eE][-+]?[0-9]+)?$/) { print "unreadable"; exit 0 }
  if (kind == "int") printf "%d\n", value
  else printf "%.1f\n", value
}
AWK

awk_program() {  # <program body>: the shared helpers followed by the body
  printf '%s\n%s\n' "$AWK_SERIES" "$1"
}

# The rows of the thresholds file, one tab-separated line each. A row that is not eight non-empty columns
# makes the file broken, and so does a file with no rows.
threshold_rows() {
  tr -d '\r' <"$THRESHOLDS" | awk -F'\t' '
    /^#/ || /^[ \t]*$/ { next }
    NF != 8 { print "threshold row " NR " has " NF " columns, not 8" > "/dev/stderr"; bad = 1; next }
    $1 == "" || $2 == "" || $3 == "" || $4 == "" || $5 == "" || $6 == "" || $7 == "" || $8 == "" {
      print "threshold row " NR " has an empty column" > "/dev/stderr"; bad = 1; next
    }
    { print }
    END { exit bad }
  '
}

valid_date() {  # <YYYY-MM-DD>: a shape and month and day ranges a calendar could hold
  [[ "$1" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}$ ]] &&
    (( 10#${1:5:2} >= 1 && 10#${1:5:2} <= 12 && 10#${1:8:2} >= 1 && 10#${1:8:2} <= 31 ))
}

valid_date "$TODAY" || {
  echo "MP_ACCEPTANCE_TODAY must be a YYYY-MM-DD date" >&2
  exit 2
}
[[ -r "$THRESHOLDS" ]] || {
  echo "acceptance thresholds file is missing" >&2
  exit 2
}
WORK="$(mktemp -d "${TMPDIR:-/tmp}/acceptance-live-metrics.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
if ! threshold_rows >"$WORK/rows.tsv"; then
  echo "acceptance thresholds file is malformed" >&2
  exit 2
fi
[[ -s "$WORK/rows.tsv" ]] || {
  echo "acceptance thresholds file has no rows" >&2
  exit 2
}

fetch() {  # <url>: the body on stdout; a non-zero status when the endpoint cannot be read
  case "$1" in
    file://*) cat -- "${1#file://}" ;;
    *) curl -fsS --max-time 5 -- "$1" ;;
  esac
}

scrape() {  # <name> <url>: read the endpoint once per run; succeeds when it was read
  local file="$WORK/$1.metrics"
  if [[ ! -f "$file.state" ]]; then
    if fetch "$2" >"$file" 2>/dev/null; then
      echo ok >"$file.state"
    else
      echo failed >"$file.state"
    fi
  fi
  [[ "$(cat "$file.state")" == ok ]]
}

days_between() {  # <from YYYY-MM-DD> <to YYYY-MM-DD>: whole days from the first date to the second
  awk -v a="$1" -v b="$2" '
    function civil(s,   p, y, m, d, era, yoe, doy, doe) {
      split(s, p, "-")
      y = p[1] + 0; m = p[2] + 0; d = p[3] + 0
      if (m <= 2) y -= 1
      era = int((y >= 0 ? y : y - 399) / 400)
      yoe = y - era * 400
      doy = int((153 * (m > 2 ? m - 3 : m + 9) + 2) / 5) + d - 1
      doe = yoe * 365 + int(yoe / 4) - int(yoe / 100) + doy
      return era * 146097 + doe - 719468
    }
    BEGIN { print civil(b) - civil(a) }
  '
}

measure_gateway() {  # ratio | p95
  if ! scrape gateway "$GATEWAY_METRICS_URL"; then
    echo unreadable
    return 0
  fi
  if [[ "$1" == ratio ]]; then
    awk "$(awk_program "$AWK_RATIO")" "$WORK/gateway.metrics"
  else
    awk "$(awk_program "$AWK_P95")" "$WORK/gateway.metrics"
  fi
}

measure_gauge() {  # <metric name> <int|float>
  if ! scrape session "$SESSION_METRICS_URL"; then
    echo unreadable
    return 0
  fi
  awk -v want="$1" -v kind="$2" "$(awk_program "$AWK_GAUGE")" "$WORK/session.metrics"
}

measure_zdr_days() {
  local policy="${MODEL_PLANE_RUNTIME_PUBLIC_POLICY_FILE:-}" report days
  if [[ -z "$policy" || ! -f "$RELEASE_TOOL" ]]; then
    echo unreadable
    return 0
  fi
  if ! report="$(bash "$RELEASE_TOOL" check-zdr-attestation "$policy" --today "$TODAY" 2>/dev/null)"; then
    echo unreadable
    return 0
  fi
  # Each claimed surface reports its days left; the closest review-by date decides.
  days="$(printf '%s\n' "$report" | sed -n 's/.*(\([0-9][0-9]*\) days left).*/\1/p' |
    awk 'NR == 1 || $1 + 0 < min { min = $1 + 0 } END { if (NR > 0) print min }')"
  if [[ -n "$days" ]]; then
    echo "$days"
  else
    echo unreadable
  fi
}

measure_rehearsal_age() {
  local newest name day first age
  newest="$(ls -1 "$REHEARSAL_EVIDENCE_DIR"/rollback-rehearsal-[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]*.txt 2>/dev/null | sort | tail -n 1 || true)"
  if [[ -z "$newest" ]]; then
    echo unreadable
    return 0
  fi
  name="$(basename "$newest")"
  day="${name#rollback-rehearsal-}"
  day="${day:0:10}"
  if ! valid_date "$day"; then
    echo unreadable
    return 0
  fi
  first="$(head -n 1 "$newest" 2>/dev/null | tr -d '\r' || true)"
  if [[ "$first" != *"result PASSED"* ]]; then
    echo failed
    return 0
  fi
  age="$(days_between "$day" "$TODAY")"
  if (( age < 0 )); then
    echo failed
  else
    echo "$age"
  fi
}

has_live_source() {  # <id>
  [[ "$LIVE_IDS" == *" $1 "* ]]
}

measure_value() {  # <live id>: its value, unreadable or failed
  case "$1" in
    gateway_error_ratio_per_mille) measure_gateway ratio ;;
    gateway_p95_latency_ms) measure_gateway p95 ;;
    audit_outbox_terminal_rows) measure_gauge mp_session_audit_outbox_terminal int ;;
    audit_delivery_lag_seconds) measure_gauge mp_session_audit_outbox_oldest_pending_seconds float ;;
    zdr_attestation_days_to_review) measure_zdr_days ;;
    rollback_rehearsal_age_days) measure_rehearsal_age ;;
  esac
}

meets() {  # <value> <operator> <target>: succeeds when the value meets the target
  awk -v v="$1" -v op="$2" -v t="$3" 'BEGIN {
    if (op == "<=") ok = (v + 0 <= t + 0)
    else if (op == "==") ok = (v + 0 == t + 0)
    else ok = (v + 0 >= t + 0)
    exit (ok ? 0 : 1)
  }'
}

run_measure() {
  local id unit target operator owner src stop status value
  local measured=0 unreadable=0
  while IFS=$'\t' read -r id unit target operator owner src stop status; do
    has_live_source "$id" || continue
    value="$(measure_value "$id")"
    printf '%s %s %s\n' "$id" "$value" "$unit"
    measured=$((measured + 1))
    case "$value" in
      unreadable | failed) unreadable=$((unreadable + 1)) ;;
    esac
  done <"$WORK/rows.tsv"
  if [[ "$measured" -eq 0 ]]; then
    echo "no threshold row has a live source" >&2
    return 1
  fi
  [[ "$unreadable" -eq 0 ]]
}

run_check() {
  local id unit target operator owner src stop status value
  local rows=0 approved=0 passing=0 failing=0 blocked=0
  while IFS=$'\t' read -r id unit target operator owner src stop status; do
    rows=$((rows + 1))
    case "$status" in
      approved)
        approved=$((approved + 1))
        if [[ "$target" == unset ]]; then
          echo "INVALID $id an unset target can only be proposed"
          failing=$((failing + 1))
          continue
        fi
        case "$operator" in
          "<=" | "==" | ">=") ;;
          *)
            echo "INVALID $id operator $operator is not one the contract allows"
            failing=$((failing + 1))
            continue
            ;;
        esac
        if ! [[ "$target" =~ ^[0-9]+$ ]]; then
          echo "INVALID $id target $target is not an integer"
          failing=$((failing + 1))
          continue
        fi
        if ! has_live_source "$id"; then
          echo "NO-SOURCE $id"
          failing=$((failing + 1))
          continue
        fi
        value="$(measure_value "$id")"
        case "$value" in
          unreadable)
            echo "UNREADABLE $id"
            failing=$((failing + 1))
            ;;
          failed)
            echo "FAIL $id failed $unit"
            failing=$((failing + 1))
            ;;
          *)
            if meets "$value" "$operator" "$target"; then
              echo "PASS $id $value $unit"
              passing=$((passing + 1))
            else
              echo "FAIL $id $value $unit"
              failing=$((failing + 1))
            fi
            ;;
        esac
        ;;
      proposed | revoked)
        if [[ "$status" == revoked && "$target" == unset ]]; then
          echo "INVALID $id an unset target can only be proposed"
          failing=$((failing + 1))
          continue
        fi
        echo "BLOCKED $status $id"
        blocked=$((blocked + 1))
        if [[ "$target" == unset ]]; then
          if has_live_source "$id"; then
            value="$(measure_value "$id")"
          else
            value=no-source
          fi
          echo "BASELINE $id $value"
        fi
        ;;
      *)
        echo "INVALID $id unknown status $status"
        failing=$((failing + 1))
        ;;
    esac
  done <"$WORK/rows.tsv"
  echo "acceptance live metrics: $rows rows, $approved approved, $passing passing, $failing failing, $blocked not approved"
  [[ "$failing" -eq 0 ]] || return 1
  [[ "$blocked" -eq 0 ]] || return 3
}

rc=0
if [[ "$1" == --measure ]]; then
  run_measure || rc=$?
else
  run_check || rc=$?
fi
exit "$rc"
