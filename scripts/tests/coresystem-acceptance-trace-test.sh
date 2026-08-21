#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
VALIDATOR="$ROOT_DIR/scripts/coresystem-acceptance-trace.sh"
test -x "$VALIDATOR"

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

cat >"$tmp_dir/valid.jsonl" <<'EOF'
{"sequence":1,"trace_id":"trace-1","stage":"space_resolved","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":2,"trace_id":"trace-1","stage":"bff_authorized","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":3,"trace_id":"trace-1","stage":"control_decision","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":4,"trace_id":"trace-1","stage":"owner_reserved","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":5,"trace_id":"trace-1","stage":"fault_injected","operation_id":"op-1","idempotency_key":"idem-1","fault":"revocation","result":"denied"}
{"sequence":6,"trace_id":"trace-1","stage":"fault_injected","operation_id":"op-1","idempotency_key":"idem-1","fault":"duplicate","result":"reconciled"}
{"sequence":7,"trace_id":"trace-1","stage":"fault_injected","operation_id":"op-1","idempotency_key":"idem-1","fault":"crash_after_submit","result":"unknown_outcome"}
{"sequence":8,"trace_id":"trace-1","stage":"fault_injected","operation_id":"op-1","idempotency_key":"idem-1","fault":"timeout","result":"unknown_outcome"}
{"sequence":9,"trace_id":"trace-1","stage":"fault_injected","operation_id":"op-1","idempotency_key":"idem-1","fault":"provider_loss","result":"unknown_outcome"}
{"sequence":10,"trace_id":"trace-1","stage":"owner_receipt","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted","receipt_id":"receipt-1"}
{"sequence":11,"trace_id":"trace-1","stage":"activity_projected","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted","receipt_id":"receipt-1"}
{"sequence":12,"trace_id":"trace-1","stage":"reconciled","operation_id":"op-1","idempotency_key":"idem-1","result":"reconciled","receipt_id":"receipt-1"}
EOF
"$VALIDATOR" "$tmp_dir/valid.jsonl" >/dev/null

cat >"$tmp_dir/secret.jsonl" <<'EOF'
{"sequence":1,"trace_id":"trace-1","stage":"space_resolved","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted","metadata":{"headers":{"authorization":"Bearer hidden"}}}
EOF
if "$VALIDATOR" "$tmp_dir/secret.jsonl" >/dev/null 2>&1; then
  echo "secret-bearing trace was accepted" >&2
  exit 1
fi

cat >"$tmp_dir/missing-fault.jsonl" <<'EOF'
{"sequence":1,"trace_id":"trace-1","stage":"space_resolved","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":2,"trace_id":"trace-1","stage":"bff_authorized","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":3,"trace_id":"trace-1","stage":"control_decision","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":4,"trace_id":"trace-1","stage":"owner_reserved","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted"}
{"sequence":5,"trace_id":"trace-1","stage":"owner_receipt","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted","receipt_id":"receipt-1"}
{"sequence":6,"trace_id":"trace-1","stage":"activity_projected","operation_id":"op-1","idempotency_key":"idem-1","result":"accepted","receipt_id":"receipt-1"}
{"sequence":7,"trace_id":"trace-1","stage":"reconciled","operation_id":"op-1","idempotency_key":"idem-1","result":"reconciled","receipt_id":"receipt-1"}
EOF
if "$VALIDATOR" "$tmp_dir/missing-fault.jsonl" >/dev/null 2>&1; then
  echo "incomplete acceptance trace was accepted" >&2
  exit 1
fi

cat >"$tmp_dir/missing-reconciliation.jsonl" <<'EOF'
{"sequence":1,"trace_id":"trace-2","stage":"space_resolved","operation_id":"op-2","idempotency_key":"idem-2","result":"accepted"}
{"sequence":2,"trace_id":"trace-2","stage":"bff_authorized","operation_id":"op-2","idempotency_key":"idem-2","result":"accepted"}
{"sequence":3,"trace_id":"trace-2","stage":"control_decision","operation_id":"op-2","idempotency_key":"idem-2","result":"accepted"}
{"sequence":4,"trace_id":"trace-2","stage":"owner_reserved","operation_id":"op-2","idempotency_key":"idem-2","result":"accepted"}
{"sequence":5,"trace_id":"trace-2","stage":"fault_injected","operation_id":"op-2","idempotency_key":"idem-2","fault":"revocation","result":"denied"}
{"sequence":6,"trace_id":"trace-2","stage":"fault_injected","operation_id":"op-2","idempotency_key":"idem-2","fault":"duplicate","result":"reconciled"}
{"sequence":7,"trace_id":"trace-2","stage":"fault_injected","operation_id":"op-2","idempotency_key":"idem-crash","fault":"crash_after_submit","result":"unknown_outcome"}
{"sequence":8,"trace_id":"trace-2","stage":"fault_injected","operation_id":"op-2","idempotency_key":"idem-timeout","fault":"timeout","result":"unknown_outcome"}
{"sequence":9,"trace_id":"trace-2","stage":"fault_injected","operation_id":"op-2","idempotency_key":"idem-provider","fault":"provider_loss","result":"unknown_outcome"}
{"sequence":10,"trace_id":"trace-2","stage":"owner_receipt","operation_id":"op-2","idempotency_key":"idem-2","result":"accepted","receipt_id":"receipt-2"}
{"sequence":11,"trace_id":"trace-2","stage":"activity_projected","operation_id":"op-2","idempotency_key":"idem-2","result":"accepted","receipt_id":"receipt-2"}
{"sequence":12,"trace_id":"trace-2","stage":"reconciled","operation_id":"op-2","idempotency_key":"idem-crash","result":"reconciled","receipt_id":"receipt-2"}
EOF
if "$VALIDATOR" "$tmp_dir/missing-reconciliation.jsonl" >/dev/null 2>&1; then
  echo "ambiguous fault without matching reconciliation was accepted" >&2
  exit 1
fi

echo "CoreSystem acceptance trace validator contract: ok"

THRESHOLD_TEST="$ROOT_DIR/apps/Model Plane/scripts/tests/acceptance-thresholds-test.sh"
test -x "$THRESHOLD_TEST"
"$THRESHOLD_TEST" >/dev/null
