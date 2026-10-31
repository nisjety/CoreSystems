#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
THRESHOLDS="${ACCEPTANCE_THRESHOLDS_FILE:-$ROOT_DIR/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv}"

# --self-test: the validator below must reject each broken copy of the real file.
if [[ "${1:-}" == "--self-test" ]]; then
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/acceptance-thresholds-test.XXXXXX")"
  trap 'rm -rf "$tmp"' EXIT
  expect_rejected() {
    local name="$1" awk_program="$2"
    awk -F'\t' -v OFS='\t' "$awk_program" "$THRESHOLDS" >"$tmp/$name.tsv"
    if ACCEPTANCE_THRESHOLDS_FILE="$tmp/$name.tsv" bash "${BASH_SOURCE[0]}" >/dev/null 2>&1; then
      echo "acceptance thresholds self-test: $name was accepted" >&2
      exit 1
    fi
  }
  expect_rejected unknown_unit '$1 == "rollback_rto_seconds" { $2 = "furlongs" } 1'
  expect_rejected unset_but_approved '$1 == "gateway_p95_latency_ms" { sub(/proposed/, "approved", $8) } 1'
  expect_rejected fixed_operator_changed '$1 == "audit_outbox_terminal_rows" { $4 = "<=" } 1'
  expect_rejected floor_became_ceiling '$1 == "zdr_attestation_days_to_review" { $4 = "<=" } 1'
  expect_rejected non_integer_target '$1 == "audit_delivery_lag_seconds" { $3 = "soon" } 1'
  expect_rejected zero_seconds '$1 == "decision_ttl_seconds" { $3 = "0" } 1'
  expect_rejected missing_row '$1 != "rollback_rehearsal_age_days"'
  ACCEPTANCE_THRESHOLDS_FILE="$THRESHOLDS" bash "${BASH_SOURCE[0]}" >/dev/null
  echo "acceptance thresholds self-test: ok"
  exit 0
fi

if [[ ! -r "$THRESHOLDS" ]]; then
  echo "acceptance thresholds artifact is missing" >&2
  exit 1
fi

python3 - "$THRESHOLDS" <<'PY'
import csv
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
rows = []
with path.open(newline="", encoding="utf-8") as handle:
    for row in csv.reader((line for line in handle if not line.startswith("#")), delimiter="\t"):
        if not row:
            continue
        if len(row) != 8:
            raise SystemExit(f"invalid threshold column count: {len(row)}")
        rows.append(row)

if not rows:
    raise SystemExit("threshold artifact is empty")

required = {
    "decision_ttl_seconds",
    "decision_future_skew_seconds",
    "scheduled_step_decision_ttl_seconds",
    "delivery_claim_lease_seconds",
    "delivery_callback_max_skew_seconds",
    "unknown_reconciliation_stale_after_seconds",
    "provider_receipt_deadline_seconds",
    "ha_recovery_window_seconds",
    "zdr_residual_content_bytes",
    "gateway_error_ratio_per_mille",
    "gateway_p95_latency_ms",
    "canary_observation_window_seconds",
    "audit_outbox_terminal_rows",
    "audit_delivery_lag_seconds",
    "zdr_attestation_days_to_review",
    "rollback_rehearsal_age_days",
    "rollback_rto_seconds",
}
# Rows whose operator is fixed by what they measure: a ceiling, a floor or an exact count.
fixed_operator = {
    "audit_outbox_terminal_rows": "==",
    "zdr_residual_content_bytes": "==",
    "canary_observation_window_seconds": ">=",
    "zdr_attestation_days_to_review": ">=",
}
units = {"seconds", "milliseconds", "bytes", "days", "rows", "per_mille"}
seen = set()
for row in rows:
    ident, unit, target, operator, owner, source, stop_rule, status = row
    if not ident or ident in seen:
        raise SystemExit(f"missing or duplicate threshold id: {ident!r}")
    seen.add(ident)
    if unit not in units:
        raise SystemExit(f"invalid unit for {ident}: {unit}")
    if operator not in {"<=", "==", ">="}:
        raise SystemExit(f"invalid operator for {ident}: {operator}")
    if ident in fixed_operator and operator != fixed_operator[ident]:
        raise SystemExit(f"invalid operator for {ident}: {operator}")
    if target == "unset":
        # Not asserted until the 7-day baseline exists; an unset value can never be approved.
        if status != "proposed":
            raise SystemExit(f"unset target for {ident} must stay proposed")
    else:
        try:
            value = int(target)
        except ValueError:
            raise SystemExit(f"non-integer target for {ident}: {target}")
        if value < 0 or (unit in {"seconds", "milliseconds", "days"} and value == 0):
            raise SystemExit(f"invalid target for {ident}: {target}")
    if not owner.strip() or not source.strip() or not stop_rule.strip():
        raise SystemExit(f"incomplete threshold metadata for {ident}")
    if status not in {"proposed", "approved", "revoked"}:
        raise SystemExit(f"invalid status for {ident}: {status}")

missing = sorted(required - seen)
if missing:
    raise SystemExit("missing threshold ids: " + ",".join(missing))

print(f"acceptance threshold contract: {len(rows)} rows valid")
if all(row[7] == "approved" for row in rows):
    print("acceptance threshold state: approved")
else:
    print("acceptance threshold state: proposed (promotion must remain blocked)")
PY
