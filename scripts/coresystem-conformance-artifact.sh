#!/usr/bin/env bash
set -euo pipefail

# Capture one canonical, secret-free conformance artifact. The underlying
# conformance command remains read-only; this wrapper persists its JSON result
# at an explicit path and refuses to overwrite an existing artifact.

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CONFORMANCE="${CORESYSTEM_CONFORMANCE_COMMAND:-$ROOT_DIR/scripts/coresystem-conformance.sh}"

usage() {
  cat <<'EOF'
Usage: scripts/coresystem-conformance-artifact.sh OUTPUT_PATH

Capture the read-only CoreSystem conformance report as an immutable JSON
artifact. OUTPUT_PATH must not already exist. The command never creates,
rotates, prints, or transmits credential values.

The conformance status is preserved: exit 0 when ready, 2 when blocked, and
another non-zero status for an invalid invocation or malformed report.
EOF
}

if [[ $# -ne 1 || "$1" == "--help" || "$1" == "-h" ]]; then
  usage
  [[ $# -eq 1 ]] && exit 0
  exit 64
fi

output_path="$1"
[[ -x "$CONFORMANCE" ]] || {
  echo "missing executable: $CONFORMANCE" >&2
  exit 1
}

temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/coresystem-conformance-artifact.XXXXXX")"
trap 'rm -rf "$temporary_dir"' EXIT
report_path="$temporary_dir/report.json"

set +e
"$CONFORMANCE" --json >"$report_path"
conformance_status=$?
set -e

if [[ "$conformance_status" -ne 0 && "$conformance_status" -ne 2 ]]; then
  echo "conformance command failed with status $conformance_status" >&2
  exit "$conformance_status"
fi

# Refuse to persist anything that resembles a secret assignment, even if a
# future child probe accidentally violates the child command's redaction rule.
if grep -Eq '(TOKEN|SECRET|PRIVATE_KEY|BASE64)[A-Z_]*=[^[:space:]]+' "$report_path" ||
  grep -Eq '"(TOKEN|SECRET|PRIVATE_KEY|BASE64)[A-Z_]*":"[^"]+"' "$report_path"; then
  echo "refusing to persist secret-shaped conformance output" >&2
  exit 1
fi

python3 - "$report_path" "$output_path" <<'PY'
import json
import pathlib
import sys

report_path = pathlib.Path(sys.argv[1])
output_path = pathlib.Path(sys.argv[2])

try:
    report = json.loads(report_path.read_text())
except (OSError, json.JSONDecodeError) as exc:
    raise SystemExit(f"invalid conformance JSON: {exc}")

required = {
    "report_version",
    "status",
    "generated_at",
    "expires_at",
    "source_revision",
    "candidate",
    "rollback",
    "checks",
    "blockers",
}
missing = sorted(required.difference(report))
if missing:
    raise SystemExit(f"conformance JSON missing fields: {', '.join(missing)}")
if report["report_version"] != "1":
    raise SystemExit(f"unsupported conformance report version: {report['report_version']}")
if report["status"] not in {"ready", "blocked"}:
    raise SystemExit(f"invalid conformance status: {report['status']}")
if not isinstance(report["checks"], dict) or not isinstance(report["blockers"], list):
    raise SystemExit("conformance checks/blockers have invalid shapes")

artifact = {
    "artifact_version": "1",
    "artifact_kind": "coresystem.conformance",
    "generated_at": report["generated_at"],
    "expires_at": report["expires_at"],
    "source_revision": report["source_revision"],
    "status": report["status"],
    "checks": report["checks"],
    "blockers": report["blockers"],
    "candidate": {
        "state": report["candidate"],
        "manifest_reference": None,
    },
    "rollback": {
        "state": report["rollback"],
        "manifest_reference": None,
    },
    "release_inputs": {
        "image_digests": {},
        "config_digest": None,
        "migration_digest": None,
        "authority_key_references": [],
        "policy_hash": None,
        "catalog_hash": None,
        "feature_flags": {},
    },
    "evidence": [
        {
            "owner": "CoreSystem release operator",
            "probe": "scripts/coresystem-conformance.sh --json",
            "class": "source/test/integration",
            "result": report["status"],
            "receipt_ids": [],
            "rollback_reference": None,
        }
    ],
}

output_path.parent.mkdir(parents=False, exist_ok=True)
try:
    with output_path.open("x", encoding="utf-8") as handle:
        json.dump(artifact, handle, indent=2, sort_keys=True)
        handle.write("\n")
except FileExistsError:
    raise SystemExit(f"refusing to overwrite existing artifact: {output_path}")
except OSError as exc:
    raise SystemExit(f"unable to write artifact: {exc}")
PY

exit "$conformance_status"
