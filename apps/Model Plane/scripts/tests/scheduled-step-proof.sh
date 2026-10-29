#!/usr/bin/env bash
set -euo pipefail

# Reproducible source/integration proof for model.schedule.step v1. This is a
# dev/test harness only: it uses in-memory Temporal workflow tests, focused
# service tests, and a disposable PostgreSQL container. It never reads,
# creates, rotates, or injects service credentials and never enables a model
# capability.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
GO_DIR="$ROOT_DIR/go"
RUST_DIR="$ROOT_DIR"
SESSION_RECEIPT_PROOF="$ROOT_DIR/scripts/tests/scheduled-step-receipt-postgres-test.sh"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

cd "$GO_DIR"

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

run_proof orchestrator_workflow_retry_unknown \
  go test -count=1 ./services/orchestrator-core/cmd/workflows \
  -run 'TestScheduledRun_(UsesPreparedThreadAndCompletes|UnknownOutcomeStopsWithoutAnotherStep|RetryKeepsTheSameStepIdempotencyTuple)$'

run_proof orchestrator_activity_handoff \
  go test -count=1 ./services/orchestrator-core/cmd/activities \
  -run 'Test(ExecuteScheduledStepActivityForwardsExactIntentAndDecision|ExecuteScheduledStepActivityStopsOnUnknownOutcome|ExecuteScheduledStepActivityStopsOnFailedOutcome|StartScheduledRunActivity_RejectsSessionCoreReceiptMismatch)$'

run_proof execution_core_scheduled_contract \
  cargo test --manifest-path "$RUST_DIR/rust/Cargo.toml" -p execution-core scheduled --no-fail-fast

run_proof session_postgres_receipt_ledger \
  "$SESSION_RECEIPT_PROOF"

printf 'scheduled-step proof: ok\n'
