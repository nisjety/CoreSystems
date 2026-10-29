#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
THRESHOLDS="$ROOT_DIR/docs/CORESYSTEM_ACCEPTANCE_THRESHOLDS.tsv"

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
}
seen = set()
for row in rows:
    ident, unit, target, operator, owner, source, stop_rule, status = row
    if not ident or ident in seen:
        raise SystemExit(f"missing or duplicate threshold id: {ident!r}")
    seen.add(ident)
    if unit not in {"seconds", "bytes"}:
        raise SystemExit(f"invalid unit for {ident}: {unit}")
    try:
        value = int(target)
    except ValueError:
        raise SystemExit(f"non-integer target for {ident}: {target}")
    if value < 0 or (unit == "seconds" and value == 0):
        raise SystemExit(f"invalid target for {ident}: {target}")
    if operator not in {"<=", "=="}:
        raise SystemExit(f"invalid operator for {ident}: {operator}")
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
