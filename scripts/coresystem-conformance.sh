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
FORMAT=human

usage() {
  cat <<'EOF'
Usage: scripts/coresystem-conformance.sh [--json]

Emit a read-only, secret-free CoreSystem release/conformance report. The
command does not create, rotate, print, or transmit credential values.
Use scripts/coresystem-conformance-artifact.sh OUTPUT_PATH when a persisted,
immutable evidence record is required.

Exit status:
  0  requested conformance profile passed
  2  fail-closed or incomplete (named blockers are reported)
  64 invalid arguments
EOF
}

case "${1:-}" in
  "") ;;
  --json) FORMAT=json ;;
  --help|-h) usage; exit 0 ;;
  *) usage >&2; exit 64 ;;
esac
if [[ $# -gt 1 ]]; then
  usage >&2
  exit 64
fi

utc_now="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
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

candidate_state=unknown
rollback_state=unknown
if [[ -f "$EVIDENCE_FILE" ]]; then
  claim="$(rg -m1 '^\| Release artifact v3 captures immutable inputs' "$EVIDENCE_FILE" || true)"
  if [[ "$claim" == *"| absent | absent |"* ]]; then
    candidate_state=absent
    rollback_state=absent
    add_blocker candidate_artifact_missing
    add_blocker rollback_artifact_missing
  elif [[ -n "$claim" ]]; then
    candidate_state=recorded
    rollback_state=recorded
  fi
fi
add_check candidate_artifact "$candidate_state"
add_check rollback_artifact "$rollback_state"

ledger_status=missing
if [[ -f "$CAPABILITY_LEDGER" && -x "$CAPABILITY_LEDGER_TEST" ]]; then
  set +e
  ledger_output="$(bash "$CAPABILITY_LEDGER_TEST" 2>&1)"
  ledger_rc=$?
  set -e
  if [[ "$ledger_rc" -eq 0 ]]; then
    if rg -q $'\tsource_only\t' "$CAPABILITY_LEDGER"; then
      ledger_status=source_only
      add_blocker capability_promotion_unproven
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
    if rg -q $'\tapproved\t' "$ACCEPTANCE_THRESHOLDS" && ! rg -q $'\t(proposed|revoked)\t' "$ACCEPTANCE_THRESHOLDS"; then
      threshold_status=approved
    else
      threshold_status=proposed
      add_blocker acceptance_thresholds_unapproved
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
add_check dev_runtime_preflight "$preflight_status"

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
add_check cross_plane_preflight "$cross_preflight_status"

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
  printf '{"report_version":"1","status":"%s","generated_at":"%s","expires_at":"%s","source_revision":"%s","candidate":"%s","rollback":"%s","checks":{' \
    "$status" "$utc_now" "$expiry" "$source_revision" "$candidate_state" "$rollback_state"
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
  printf ']}\n'
else
  printf 'CoreSystem conformance (R-0); generated=%s expires=%s\n' "$utc_now" "$expiry"
  printf 'source_revision=%s\n' "$source_revision"
  printf 'candidate=%s rollback=%s\n' "$candidate_state" "$rollback_state"
  for check in "${checks[@]}"; do
    printf 'CHECK %s\n' "$check"
  done
  printf 'STATUS %s\n' "$status"
  if ((${#blockers[@]} > 0)); then
    printf 'BLOCKER %s\n' "${blockers[@]}"
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
