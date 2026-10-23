#!/usr/bin/env bash
set -euo pipefail

# Read-only CoreSystem conformance report. This is an operator contract, not a
# deployer: it reports named evidence gaps and never prints or creates secret
# values. Plane probes remain evidence checks, not release promotion.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="$ROOT_DIR/apps/Model Plane"
EVIDENCE_FILE="$MODEL_DIR/docs/MODEL_PLANE_RELEASE_EVIDENCE.md"
PREFLIGHT="$MODEL_DIR/scripts/tests/dev-runtime-preflight.sh"
CROSS_PREFLIGHT="$ROOT_DIR/scripts/coresystem-cross-plane-preflight.sh"
CAPABILITY_LEDGER="$MODEL_DIR/docs/CAPABILITY_PROMOTION_LEDGER.tsv"
CAPABILITY_LEDGER_TEST="$MODEL_DIR/scripts/tests/capability-promotion-ledger-test.sh"
ACCEPTANCE_THRESHOLDS="$MODEL_DIR/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv"
ACCEPTANCE_THRESHOLDS_TEST="$MODEL_DIR/scripts/tests/acceptance-thresholds-test.sh"
RELEASE_TOOL="${MODEL_PLANE_RELEASE_ARTIFACT_SCRIPT:-$MODEL_DIR/scripts/release-artifact.sh}"
WAIVERS="${MODEL_PLANE_RELEASE_GATE_WAIVERS_FILE:-$MODEL_DIR/docs/RELEASE_GATE_WAIVERS.tsv}"
REHEARSAL_EVIDENCE_DIR="${CORESYSTEM_ROLLBACK_EVIDENCE_DIR:-$ROOT_DIR/docs/compliance/evidence}"
# Used while the thresholds file has no approved value for rollback_rehearsal_age_days.
DEFAULT_REHEARSAL_MAX_AGE_DAYS=30
FORMAT=human
PROFILE="${CORESYSTEM_CONFORMANCE_PROFILE:-full}"

usage() {
  cat <<'EOF'
Usage: scripts/coresystem-conformance.sh [--json] [--profile runtime|full]

Emit a read-only, secret-free CoreSystem release/conformance report. The
command does not create, rotate, print, or transmit credential values.
Use scripts/coresystem-conformance-artifact.sh OUTPUT_PATH when a persisted,
immutable evidence record is required.

Profiles (default full; CORESYSTEM_CONFORMANCE_PROFILE sets the default):
  runtime  the Model Plane runtime gate: clean tree, required files, signed
           candidate and rollback artifacts, a passed and recent rollback
           rehearsal, a valid thresholds file and capability ledger. A ready
           runtime report is not promotion.
  full     runtime plus approved thresholds, a fully promoted capability
           ledger and the dev-runtime and cross-plane preflights.

Artifact inputs, by path only: MODEL_PLANE_CANDIDATE_ARTIFACT_DIR,
MODEL_PLANE_ROLLBACK_ARTIFACT_DIR, MODEL_PLANE_ARTIFACT_VERIFY_KEY and
optionally MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY. Each artifact is verified
by scripts/release-artifact.sh against the trusted key; a markdown claim is not
evidence. A blocker can be waived only by an unexpired row of
apps/Model Plane/docs/RELEASE_GATE_WAIVERS.tsv.

Exit status:
  0  requested conformance profile passed
  2  fail-closed or incomplete (named blockers are reported)
  64 invalid arguments
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --json) FORMAT=json; shift ;;
    --profile)
      [[ $# -ge 2 ]] || { usage >&2; exit 64; }
      PROFILE="$2"
      shift 2
      ;;
    --help|-h) usage; exit 0 ;;
    *) usage >&2; exit 64 ;;
  esac
done
case "$PROFILE" in
  runtime|full) ;;
  *) usage >&2; exit 64 ;;
esac

utc_now="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
today="${CORESYSTEM_CONFORMANCE_TODAY:-$(date -u '+%Y-%m-%d')}"
if expiry="$(date -u -v+15M '+%Y-%m-%dT%H:%M:%SZ' 2>/dev/null)"; then
  :
else
  expiry="$(date -u -d '+15 minutes' '+%Y-%m-%dT%H:%M:%SZ')"
fi

source_revision="unknown"
if command -v git >/dev/null 2>&1; then
  source_revision="$(git -C "$ROOT_DIR" rev-parse HEAD 2>/dev/null || printf 'unknown')"
fi

blockers=()
checks=()
waived=()
add_blocker() { blockers+=("$1"); }
add_check() { checks+=("$1=$2"); }

if [[ -n "$(git -C "$ROOT_DIR" status --porcelain 2>/dev/null || true)" ]]; then
  add_check dirty_worktree blocked
  add_blocker dirty_worktree
else
  add_check dirty_worktree pass
fi

required_files=(
  "apps/Model Plane/docs/MODEL_PLANE_RELEASE_EVIDENCE.md"
  "apps/Model Plane/docs/DEV_AUTHORITY_HANDOFF_MATRIX.md"
  "apps/Frontend Plane/verevonv3/docs/VEREVON_QM_COMPARISON_AND_ADOPTION_PLAN_2026-08-13.md"
  "apps/Model Plane/scripts/tests/release-contract-test.sh"
  "apps/Model Plane/scripts/tests/dev-runtime-preflight.sh"
  "scripts/coresystem-cross-plane-preflight.sh"
  "scripts/coresystem-acceptance-trace.sh"
  "apps/Model Plane/docs/CAPABILITY_PROMOTION_LEDGER.tsv"
  "apps/Model Plane/scripts/tests/capability-promotion-ledger-test.sh"
  "apps/Model Plane/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv"
  "apps/Model Plane/scripts/tests/acceptance-thresholds-test.sh"
  "apps/Model Plane/docs/RELEASE_GATE_WAIVERS.tsv"
  "apps/Model Plane/scripts/release-artifact.sh"
  "apps/Model Plane/scripts/rollback-rehearsal.sh"
  ".github/workflows/model-plane-ops-gates.yml"
  "scripts/coresystem-conformance-artifact.sh"
  "scripts/tests/coresystem-conformance-artifact-test.sh"
  "apps/Model Plane/scripts/tests/capability-health-proof.sh"
  "apps/Model Plane/scripts/tests/scheduled-step-proof.sh"
  "apps/Model Plane/scripts/tests/approval-continuation-proof.sh"
  "apps/Model Plane/scripts/tests/approval-continuation-proof-test.sh"
  "apps/Model Plane/scripts/tests/scheduled-step-config-contract-test.sh"
  "apps/Model Plane/scripts/tests/tickets-create-proof.sh"
  "apps/Model Plane/scripts/tests/tickets-create-config-contract-test.sh"
  "apps/Model Plane/scripts/tests/application-delivery-proof.sh"
  "apps/Model Plane/scripts/tests/application-delivery-proof-test.sh"
  "apps/Model Plane/scripts/tests/coresystem-qm-proof.sh"
  "apps/Model Plane/scripts/tests/coresystem-qm-proof-test.sh"
  "scripts/tests/coresystem-acceptance-trace-test.sh"
)
for relative in "${required_files[@]}"; do
  if [[ -f "$ROOT_DIR/$relative" ]]; then
    add_check "file:${relative}" present
  else
    add_check "file:${relative}" missing
    add_blocker "missing_file:${relative}"
  fi
done

# Both artifacts are verified by the release tool against a trusted signer key. The
# tool's own output is dropped: it names paths and this report stays content-free.
verify_signed_artifact() {
  local dir="$1" key="$2"
  [[ -f "$RELEASE_TOOL" ]] || return 1
  MODEL_PLANE_ARTIFACT_VERIFY_KEY="$key" bash "$RELEASE_TOOL" verify "$dir" >/dev/null 2>&1
}

candidate_state=absent
rollback_state=absent
candidate_dir="${MODEL_PLANE_CANDIDATE_ARTIFACT_DIR:-}"
rollback_dir="${MODEL_PLANE_ROLLBACK_ARTIFACT_DIR:-}"
candidate_key="${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-}"
rollback_key="${MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY:-$candidate_key}"
if [[ -z "$candidate_dir" ]]; then
  add_blocker candidate_artifact_missing
elif [[ -z "$candidate_key" ]]; then
  candidate_state=unverified
  add_blocker artifact_verify_key_missing
elif MODEL_PLANE_RELEASE_MODE=1 verify_signed_artifact "$candidate_dir" "$candidate_key"; then
  candidate_state=verified
else
  candidate_state=unverified
  add_blocker candidate_artifact_unverified
fi
if [[ -z "$rollback_dir" ]]; then
  add_blocker rollback_artifact_missing
elif [[ -z "$rollback_key" ]]; then
  rollback_state=unverified
  add_blocker artifact_verify_key_missing
elif verify_signed_artifact "$rollback_dir" "$rollback_key"; then
  rollback_state=verified
else
  rollback_state=unverified
  add_blocker rollback_artifact_unverified
fi
add_check candidate_artifact "$candidate_state"
add_check rollback_artifact "$rollback_state"

# A rehearsal result is the newest rollback-rehearsal-<date>.txt that
# scripts/rollback-rehearsal.sh wrote: it must say PASSED and be recent enough.
rehearsal_max_age="$DEFAULT_REHEARSAL_MAX_AGE_DAYS"
if [[ -f "$ACCEPTANCE_THRESHOLDS" ]]; then
  configured_age="$(tr -d '\r' <"$ACCEPTANCE_THRESHOLDS" | awk -F'\t' '$1 == "rollback_rehearsal_age_days" && $3 ~ /^[0-9]+$/ && $8 == "approved" { print $3 }')"
  [[ -z "$configured_age" ]] || rehearsal_max_age="$configured_age"
fi
rehearsal_state=missing
rehearsal_file="$(ls -1 "$REHEARSAL_EVIDENCE_DIR"/rollback-rehearsal-[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]*.txt 2>/dev/null | sort | tail -n 1 || true)"
if [[ -n "$rehearsal_file" ]]; then
  rehearsal_date="$(basename "$rehearsal_file")"
  rehearsal_date="${rehearsal_date#rollback-rehearsal-}"
  rehearsal_date="${rehearsal_date:0:10}"
  rehearsal_age="$(python3 - "$rehearsal_date" "$today" <<'PY' || true
import datetime
import sys

try:
    print((datetime.date.fromisoformat(sys.argv[2]) - datetime.date.fromisoformat(sys.argv[1])).days)
except ValueError:
    print("invalid")
PY
)"
  if ! head -n 1 "$rehearsal_file" | grep -q 'result PASSED'; then
    rehearsal_state=failed
  elif [[ ! "$rehearsal_age" =~ ^-?[0-9]+$ || "$rehearsal_age" -lt 0 ]]; then
    rehearsal_state=failed
  elif [[ "$rehearsal_age" -gt "$rehearsal_max_age" ]]; then
    rehearsal_state=stale
  else
    rehearsal_state=passed
  fi
fi
case "$rehearsal_state" in
  missing) add_blocker rollback_rehearsal_missing ;;
  failed) add_blocker rollback_rehearsal_failed ;;
  stale) add_blocker rollback_rehearsal_stale ;;
esac
add_check rollback_rehearsal "$rehearsal_state"

ledger_status=missing
if [[ -f "$CAPABILITY_LEDGER" && -x "$CAPABILITY_LEDGER_TEST" ]]; then
  set +e
  ledger_output="$(bash "$CAPABILITY_LEDGER_TEST" 2>&1)"
  ledger_rc=$?
  set -e
  if [[ "$ledger_rc" -eq 0 ]]; then
    if grep -q $'\tsource_only\t' "$CAPABILITY_LEDGER"; then
      ledger_status=source_only
      # The runtime profile gates the signed artifact, not capability promotion.
      [[ "$PROFILE" == runtime ]] || add_blocker capability_promotion_unproven
    else
      ledger_status=promoted
    fi
  else
    ledger_status=invalid
    add_blocker capability_promotion_ledger_invalid
  fi
else
  ledger_output=""
  add_blocker capability_promotion_ledger_missing
fi
add_check capability_promotion_ledger "$ledger_status"

threshold_status=missing
if [[ -f "$ACCEPTANCE_THRESHOLDS" && -x "$ACCEPTANCE_THRESHOLDS_TEST" ]]; then
  set +e
  threshold_output="$(bash "$ACCEPTANCE_THRESHOLDS_TEST" 2>&1)"
  threshold_rc=$?
  set -e
  if [[ "$threshold_rc" -eq 0 ]]; then
    # The status is the last column, so a tab-anchored grep on the file never saw it;
    # the test already reports the state of every row.
    if grep -q '^acceptance threshold state: approved$' <<<"$threshold_output"; then
      threshold_status=approved
    else
      threshold_status=proposed
      [[ "$PROFILE" == runtime ]] || add_blocker acceptance_thresholds_unapproved
    fi
  else
    threshold_status=invalid
    add_blocker acceptance_thresholds_invalid
  fi
else
  threshold_output=""
  add_blocker acceptance_thresholds_missing
fi
add_check acceptance_thresholds "$threshold_status"

# The preflights read the running dev stack and the other planes; the runtime
# profile gates the signed artifact and leaves them to the full profile.
preflight_status=skipped
preflight_output=""
cross_preflight_status=skipped
cross_preflight_output=""
if [[ "$PROFILE" == full ]]; then
  preflight_status=missing
  if [[ -x "$PREFLIGHT" ]]; then
    set +e
    preflight_output="$(bash "$PREFLIGHT" 2>&1)"
    preflight_rc=$?
    set -e
    if [[ "$preflight_rc" -eq 0 ]]; then
      preflight_status=pass
    else
      preflight_status=blocked
      add_blocker dev_runtime_preflight
    fi
  else
    preflight_output=""
    add_blocker dev_runtime_preflight_missing
  fi

  cross_preflight_status=missing
  if [[ -x "$CROSS_PREFLIGHT" ]]; then
    set +e
    cross_preflight_output="$(bash "$CROSS_PREFLIGHT" 2>&1)"
    cross_preflight_rc=$?
    set -e
    if [[ "$cross_preflight_rc" -eq 0 ]]; then
      cross_preflight_status=pass
    else
      cross_preflight_status=blocked
      add_blocker cross_plane_preflight
    fi
  else
    cross_preflight_output=""
    add_blocker cross_plane_preflight_missing
  fi
fi
add_check dev_runtime_preflight "$preflight_status"
add_check cross_plane_preflight "$cross_preflight_status"

# Waivers: an unexpired row of the waivers file removes the named blocker from the
# report and lists it under "waived". An expired row for a live blocker adds
# waiver_expired:<blocker>; a malformed file blocks everything it would have waived.
# Blockers that say the evidence is wrong (a failed signature check, an invalid
# thresholds or waiver file) cannot be waived.
apply_waivers() {
  # A missing file is already a missing_file blocker; with no file nothing is waived.
  [[ -f "$WAIVERS" ]] || return 0
  local result kind rest blocker remaining=()
  result="$(python3 - "$WAIVERS" "$PROFILE" "$today" ${blockers[@]+"${blockers[@]}"} <<'PY'
import csv
import datetime
import sys

path, profile, today, *active = sys.argv[1:]
today = datetime.date.fromisoformat(today)
MAX_DAYS = 30


def invalid(reason):
    print("invalid\t" + reason)
    raise SystemExit(0)


def unwaivable(blocker):
    return (
        blocker.endswith("_unverified")
        or blocker.startswith("waiver_expired:")
        or blocker in {"release_gate_waivers_invalid",
                       "artifact_verify_key_missing", "acceptance_thresholds_invalid",
                       "dirty_worktree"}
    )


rows = []
with open(path, newline="", encoding="utf-8") as handle:
    for row in csv.reader((l for l in handle if not l.startswith("#") and l.strip()), delimiter="\t"):
        if len(row) != 7:
            invalid("column count %d" % len(row))
        rows.append(row)

seen = set()
for blocker, profiles, owner, reason, granted, expires, evidence in rows:
    try:
        granted_on = datetime.date.fromisoformat(granted)
        expires_on = datetime.date.fromisoformat(expires)
    except ValueError:
        invalid("bad date for " + blocker)
    if not (blocker and owner.strip() and reason.strip() and evidence.strip()):
        invalid("incomplete row for " + blocker)
    if unwaivable(blocker):
        invalid("blocker cannot be waived: " + blocker)
    if not set(profiles.split(",")) <= {"runtime", "full"}:
        invalid("bad profiles for " + blocker)
    if granted_on > today or expires_on < granted_on or (expires_on - granted_on).days > MAX_DAYS:
        invalid("bad validity window for " + blocker)
    if (blocker, profiles) in seen:
        invalid("duplicate waiver for " + blocker)
    seen.add((blocker, profiles))
    if blocker not in active or profile not in profiles.split(","):
        continue
    print(("expired\t" if expires_on < today else "waived\t") + blocker + "\t" + expires)
PY
)" || { add_blocker release_gate_waivers_invalid; return 0; }
  # One bad row voids the whole file, so no waived line may be acted on before that is known.
  if grep -q $'^invalid\t' <<<"$result"; then
    add_blocker release_gate_waivers_invalid
    return 0
  fi
  while IFS=$'\t' read -r kind blocker rest; do
    case "$kind" in
      waived) waived+=("$blocker=$rest") ;;
      expired) add_blocker "waiver_expired:$blocker" ;;
    esac
  done <<<"$result"
  for blocker in ${blockers[@]+"${blockers[@]}"}; do
    local drop=0 entry
    for entry in ${waived[@]+"${waived[@]}"}; do
      [[ "${entry%%=*}" == "$blocker" ]] && drop=1
    done
    [[ "$drop" -eq 1 ]] || remaining+=("$blocker")
  done
  blockers=(${remaining[@]+"${remaining[@]}"})
}
apply_waivers
add_check release_gate_waivers "${#waived[@]}_applied"

if ((${#blockers[@]} == 0)); then
  status=ready
  exit_status=0
else
  status=blocked
  exit_status=2
fi

json_escape() {
  local value="$1"
  value="${value//\\/\\\\}"
  value="${value//\"/\\\"}"
  value="${value//$'\n'/\\n}"
  printf '%s' "$value"
}

if [[ "$FORMAT" == json ]]; then
  printf '{"report_version":"1","status":"%s","profile":"%s","generated_at":"%s","expires_at":"%s","source_revision":"%s","candidate":"%s","rollback":"%s","checks":{' \
    "$status" "$PROFILE" "$utc_now" "$expiry" "$source_revision" "$candidate_state" "$rollback_state"
  first=1
  for check in "${checks[@]}"; do
    key="${check%%=*}"
    value="${check#*=}"
    [[ "$first" -eq 1 ]] || printf ','
    first=0
    printf '"%s":"%s"' "$(json_escape "$key")" "$(json_escape "$value")"
  done
  printf '},"blockers":['
  first=1
  for blocker in "${blockers[@]}"; do
    [[ "$first" -eq 1 ]] || printf ','
    first=0
    printf '"%s"' "$(json_escape "$blocker")"
  done
  printf '],"waived":['
  first=1
  for entry in "${waived[@]}"; do
    [[ "$first" -eq 1 ]] || printf ','
    first=0
    printf '"%s"' "$(json_escape "$entry")"
  done
  printf ']}\n'
else
  printf 'CoreSystem conformance (R-0); profile=%s generated=%s expires=%s\n' "$PROFILE" "$utc_now" "$expiry"
  printf 'source_revision=%s\n' "$source_revision"
  printf 'candidate=%s rollback=%s\n' "$candidate_state" "$rollback_state"
  for check in "${checks[@]}"; do
    printf 'CHECK %s\n' "$check"
  done
  printf 'STATUS %s\n' "$status"
  if ((${#blockers[@]} > 0)); then
    printf 'BLOCKER %s\n' "${blockers[@]}"
  fi
  if ((${#waived[@]} > 0)); then
    printf 'WAIVED %s\n' "${waived[@]}"
  fi
  if [[ -n "$preflight_output" ]]; then
    printf 'DEV_PREFLIGHT_BEGIN\n'
    # The child contract is already secret-free; retain only its named state
    # lines in this aggregate report.
    while IFS= read -r line; do
      case "$line" in
        'STATUS '*|'BLOCKER '*|'CONFIG '*|'CHECKED '*|'BLOCKED '*) printf '%s\n' "$line" ;;
      esac
    done <<< "$preflight_output"
    printf 'DEV_PREFLIGHT_END\n'
  fi
  if [[ -n "$cross_preflight_output" ]]; then
    printf 'CROSS_PLANE_PREFLIGHT_BEGIN\n'
    # The child contract is already secret-free; retain only its named state
    # lines in this aggregate report.
    while IFS= read -r line; do
      case "$line" in
        'STATUS '*|'BLOCKER '*|'CONFIG '*|'CHECKED '*|'BLOCKED '*|'EVIDENCE '*) printf '%s\n' "$line" ;;
      esac
    done <<< "$cross_preflight_output"
    printf 'CROSS_PLANE_PREFLIGHT_END\n'
  fi
fi

exit "$exit_status"
