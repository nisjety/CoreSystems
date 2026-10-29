#!/usr/bin/env bash
set -euo pipefail

# Validate a secret-free, content-free CoreSystem acceptance trace produced by
# an authenticated Space → BFF → Control → owner → Activity run. This is a
# receipt-contract validator, not an effect runner: it never calls a service,
# reads credentials, or promotes a capability.

usage() {
  cat <<'EOF'
Usage: scripts/coresystem-acceptance-trace.sh TRACE.jsonl

Validate the canonical cross-plane acceptance trace. Each line must be one
JSON object with sequence, trace_id, stage, operation_id, idempotency_key,
result, and optional fault/receipt_id fields. The trace may contain several
fault attempts but all events must belong to one trace_id. The trace must
contain the ordered stages:
  space_resolved bff_authorized control_decision owner_reserved
  owner_receipt activity_projected reconciled

The trace must include revocation, duplicate, crash_after_submit, timeout, and
provider_loss fault cases. Ambiguous faults must produce unknown_outcome before
their reconciled receipt. The validator rejects credentials and content fields.
It does not establish deployed, candidate, provider/ZDR, or rollback evidence.
EOF
}

if [[ $# -ne 1 || "${1:-}" == "--help" || "${1:-}" == "-h" ]]; then
  usage >&2
  [[ $# -eq 1 ]] && exit 0
  exit 64
fi

trace_path="$1"
if [[ ! -f "$trace_path" || ! -r "$trace_path" ]]; then
  printf 'BLOCKED trace_missing=%s\n' "$trace_path" >&2
  exit 2
fi

python3 - "$trace_path" <<'PY'
import json
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
raw = path.read_bytes()
if len(raw) > 1_048_576:
    raise SystemExit("BLOCKED trace_too_large")

forbidden_keys = {
    "authorization", "token", "bearer", "secret", "password", "api_key",
    "private_key", "credential", "body", "content", "prompt", "output",
}

def forbidden_in(value):
    if isinstance(value, dict):
        for key, child in value.items():
            if str(key).lower() in forbidden_keys:
                return str(key).lower()
            found = forbidden_in(child)
            if found:
                return found
    elif isinstance(value, list):
        for child in value:
            found = forbidden_in(child)
            if found:
                return found
    elif isinstance(value, str) and value.lower().startswith("bearer "):
        return "bearer_value"
    return None

required_stages = [
    "space_resolved", "bff_authorized", "control_decision", "owner_reserved",
    "owner_receipt", "activity_projected", "reconciled",
]
required_faults = {"revocation", "duplicate", "crash_after_submit", "timeout", "provider_loss"}
ambiguous_faults = {"crash_after_submit", "timeout", "provider_loss"}
allowed_results = {"accepted", "denied", "unknown_outcome", "reconciled"}
allowed_stages = set(required_stages) | {"fault_injected"}

events = []
for line_no, line in enumerate(raw.decode("utf-8").splitlines(), 1):
    if not line.strip():
        continue
    try:
        event = json.loads(line)
    except json.JSONDecodeError as exc:
        raise SystemExit(f"BLOCKED invalid_json_line={line_no}:{exc.msg}")
    if not isinstance(event, dict):
        raise SystemExit(f"BLOCKED event_not_object_line={line_no}")
    leaked = forbidden_in(event)
    if leaked:
        raise SystemExit(f"BLOCKED forbidden_fields_line={line_no}:{leaked}")
    for key in ("sequence", "trace_id", "stage", "operation_id", "idempotency_key", "result"):
        if key not in event:
            raise SystemExit(f"BLOCKED missing_field_line={line_no}:{key}")
    if not isinstance(event["sequence"], int) or event["sequence"] < 1:
        raise SystemExit(f"BLOCKED invalid_sequence_line={line_no}")
    if not isinstance(event["stage"], str) or event["stage"] not in allowed_stages:
        raise SystemExit(f"BLOCKED invalid_stage_line={line_no}")
    for key in ("trace_id", "operation_id", "idempotency_key"):
        if not isinstance(event[key], str) or not event[key].strip() or len(event[key]) > 256:
            raise SystemExit(f"BLOCKED invalid_{key}_line={line_no}")
    if event["result"] not in allowed_results:
        raise SystemExit(f"BLOCKED invalid_result_line={line_no}")
    fault = event.get("fault", "none")
    if fault != "none" and fault not in required_faults:
        raise SystemExit(f"BLOCKED invalid_fault_line={line_no}")
    if event["stage"] in {"owner_receipt", "activity_projected", "reconciled"}:
        receipt = event.get("receipt_id")
        if not isinstance(receipt, str) or not receipt.strip() or len(receipt) > 256:
            raise SystemExit(f"BLOCKED receipt_required_line={line_no}")
    events.append(event)

if not events:
    raise SystemExit("BLOCKED trace_empty")

sequences = [event["sequence"] for event in events]
if sequences != sorted(set(sequences)):
    raise SystemExit("BLOCKED sequence_not_strictly_increasing")

trace_ids = {event["trace_id"] for event in events}
if len(trace_ids) != 1:
    raise SystemExit("BLOCKED multiple_trace_ids")

stage_positions = {stage: next(i for i, event in enumerate(events) if event["stage"] == stage)
                   for stage in required_stages}
if any(stage_positions[a] >= stage_positions[b] for a, b in zip(required_stages, required_stages[1:])):
    raise SystemExit("BLOCKED stage_order_invalid")

seen_faults = {event.get("fault") for event in events if event.get("fault", "none") != "none"}
missing_faults = sorted(required_faults - seen_faults)
if missing_faults:
    raise SystemExit(f"BLOCKED missing_faults={','.join(missing_faults)}")

for fault in ambiguous_faults:
    unknown = [event for event in events if event.get("fault") == fault and event["result"] == "unknown_outcome"]
    if not unknown:
        raise SystemExit(f"BLOCKED ambiguous_fault_without_unknown={fault}")
    for ambiguous in unknown:
        # Reconciliation must settle this exact operation/idempotency tuple;
        # a later receipt for a different attempt cannot close the uncertainty
        # window left by this fault.
        if not any(
            event["stage"] == "reconciled"
            and event["result"] == "reconciled"
            and event["sequence"] > ambiguous["sequence"]
            and event["operation_id"] == ambiguous["operation_id"]
            and event["idempotency_key"] == ambiguous["idempotency_key"]
            for event in events
        ):
            raise SystemExit(
                f"BLOCKED ambiguous_fault_without_reconciliation={fault}:"
                f"{ambiguous['operation_id']}:{ambiguous['idempotency_key']}"
            )

if not any(event.get("fault") == "revocation" and event["result"] == "denied" for event in events):
    raise SystemExit("BLOCKED revocation_not_denied")
if not any(event.get("fault") == "duplicate" and event["result"] in {"accepted", "reconciled"} for event in events):
    raise SystemExit("BLOCKED duplicate_not_idempotent")

print("CoreSystem acceptance trace contract: ok")
print("Trace validation is source/fixture evidence only; deployed release gates remain unchanged.")
PY
