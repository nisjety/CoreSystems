#!/usr/bin/env bash
set -euo pipefail

# Aggregate source/disposable proof for the current CoreSystem/QM objective.
# This is not a release or promotion command. It never creates, rotates, or
# prints credential values, and it never changes the Model capability ledger.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
CORE_ROOT="$(cd "$ROOT_DIR/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

usage() {
  cat <<'EOF'
Usage: scripts/tests/coresystem-qm-proof.sh

Run the source/disposable proofs for Capability Core R-2, scheduled-step,
approval continuation, governed tickets.create, the customer-facing Model
chat/reconnect/ZDR/approval scenarios, Application delivery, and their
empty-default configuration contracts, plus the canonical conformance-artifact
and cross-plane release-preflight contracts.
This runner never creates, rotates, or prints credential values. It does not
promote a capability and does not establish candidate, provider/ZDR, approval,
delivery, or rollback evidence.
EOF
}

if [[ "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage
  exit 0
fi
if [[ $# -ne 0 ]]; then
  usage >&2
  exit 64
fi

run_proof() {
  local name="$1"
  shift
  printf 'PROOF %s\n' "$name"
  if "$@" >"$TMP_DIR/${name}.log" 2>&1; then
    printf 'PASS  %s\n' "$name"
  else
    printf 'BLOCKED %s\n' "$name"
    sed -n '1,240p' "$TMP_DIR/${name}.log"
    exit 2
  fi
}

run_proof capability_health \
  "$ROOT_DIR/scripts/tests/capability-health-proof.sh"
run_proof scheduled_step \
  "$ROOT_DIR/scripts/tests/scheduled-step-proof.sh"
run_proof approval_continuation_contract \
  "$ROOT_DIR/scripts/tests/approval-continuation-proof-test.sh"
run_proof approval_continuation \
  "$ROOT_DIR/scripts/tests/approval-continuation-proof.sh"
run_proof tickets_create \
  "$ROOT_DIR/scripts/tests/tickets-create-proof.sh"
run_proof customer_e2e \
  "$ROOT_DIR/scripts/tests/customer-proof-e2e-test.sh"
run_proof scheduled_step_config \
  "$ROOT_DIR/scripts/tests/scheduled-step-config-contract-test.sh"
run_proof tickets_create_config \
  "$ROOT_DIR/scripts/tests/tickets-create-config-contract-test.sh"
run_proof application_delivery \
  "$ROOT_DIR/scripts/tests/application-delivery-proof.sh"
run_proof application_delivery_contract \
  "$ROOT_DIR/scripts/tests/application-delivery-proof-test.sh"
run_proof conformance_artifact_contract \
  "$CORE_ROOT/scripts/tests/coresystem-conformance-artifact-test.sh"
run_proof cross_plane_preflight_contract \
  "$CORE_ROOT/scripts/tests/coresystem-cross-plane-preflight-test.sh"
run_proof acceptance_trace_contract \
  "$CORE_ROOT/scripts/tests/coresystem-acceptance-trace-test.sh"

printf 'CoreSystem QM source/disposable proof: ok\n'
printf 'R-2 authorization and objective lanes are source-tested; capability state remains source_only.\n'
printf 'Live candidate evidence, provider/ZDR, approval-continuation, callback/HA, and rollback evidence remain required.\n'
