#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMMAND="$ROOT_DIR/scripts/coresystem-conformance-artifact.sh"

[[ -x "$COMMAND" ]] || {
  echo "missing executable: $COMMAND" >&2
  exit 1
}

temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/coresystem-conformance-artifact-test.XXXXXX")"
trap 'rm -rf "$temporary_dir"' EXIT
artifact_path="$temporary_dir/conformance.json"
fake_conformance="$temporary_dir/fake-conformance.sh"
cat >"$fake_conformance" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
[[ "${1:-}" == "--json" ]] || exit 64
printf '%s\n' '{"report_version":"1","status":"blocked","generated_at":"2026-08-17T00:00:00Z","expires_at":"2026-08-17T00:15:00Z","source_revision":"dev-revision","candidate":"absent","rollback":"absent","checks":{"dirty_worktree":"blocked"},"blockers":["dirty_worktree"]}'
exit 2
EOF
chmod +x "$fake_conformance"

set +e
output="$(CORESYSTEM_CONFORMANCE_COMMAND="$fake_conformance" "$COMMAND" "$artifact_path" 2>&1)"
status=$?
set -e

[[ "$status" -eq 2 ]] || {
  echo "expected blocked conformance capture (status 2), got $status" >&2
  printf '%s\n' "$output" >&2
  exit 1
}
[[ -s "$artifact_path" ]] || {
  echo "artifact was not written" >&2
  exit 1
}

python3 - "$artifact_path" <<'PY'
import json
import pathlib
import sys

artifact = json.loads(pathlib.Path(sys.argv[1]).read_text())
required = {
    "artifact_version",
    "artifact_kind",
    "generated_at",
    "expires_at",
    "source_revision",
    "status",
    "checks",
    "blockers",
    "candidate",
    "rollback",
    "release_inputs",
    "evidence",
}
assert required.issubset(artifact), sorted(required.difference(artifact))
assert artifact["artifact_version"] == "1"
assert artifact["artifact_kind"] == "coresystem.conformance"
assert artifact["status"] == "blocked"
assert artifact["candidate"]["state"] == "absent"
assert artifact["rollback"]["state"] == "absent"
assert artifact["release_inputs"]["image_digests"] == {}
assert artifact["release_inputs"]["authority_key_references"] == []
assert artifact["evidence"][0]["owner"] == "CoreSystem release operator"
assert artifact["evidence"][0]["receipt_ids"] == []
serialized = json.dumps(artifact)
for marker in ("TOKEN=", "SECRET=", "PRIVATE_KEY=", "BASE64="):
    assert marker not in serialized
PY

set +e
overwrite_output="$(CORESYSTEM_CONFORMANCE_COMMAND="$fake_conformance" "$COMMAND" "$artifact_path" 2>&1)"
overwrite_status=$?
set -e
[[ "$overwrite_status" -eq 1 ]] || {
  echo "expected overwrite refusal (status 1), got $overwrite_status" >&2
  printf '%s\n' "$overwrite_output" >&2
  exit 1
}
grep -q "refusing to overwrite existing artifact" <<<"$overwrite_output"

echo "CoreSystem conformance artifact contract: ok"
