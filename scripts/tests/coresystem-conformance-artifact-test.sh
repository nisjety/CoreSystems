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

# The gate itself, offline: a stub release tool stands in for scripts/release-artifact.sh, the
# evidence directory and the waivers file are temporary, and "today" is pinned. Only the
# blockers each case is about are asserted, because the dirty-tree check reads the real tree.
GATE="$ROOT_DIR/scripts/coresystem-conformance.sh"
gate_dir="$temporary_dir/gate"
mkdir -p "$gate_dir/good-candidate" "$gate_dir/good-rollback" "$gate_dir/bad-artifact" "$gate_dir/evidence"
touch "$gate_dir/good-candidate/.good" "$gate_dir/good-rollback/.good"
verify_log="$gate_dir/verify-calls.log"
cat >"$gate_dir/stub-release.sh" <<'EOF'
#!/usr/bin/env bash
# Passes `verify <dir>` for a directory holding a .good marker; logs what the gate set.
set -euo pipefail
[[ "${1:-}" == "verify" ]] || exit 64
printf '%s mode=%s key=%s\n' "$(basename "$2")" "${MODEL_PLANE_RELEASE_MODE:-0}" "${MODEL_PLANE_ARTIFACT_VERIFY_KEY:-none}" >>"$VERIFY_LOG"
[[ -f "$2/.good" ]]
EOF
export VERIFY_LOG="$verify_log"
today="2026-10-23"

# run_gate NAME [ENV=VALUE ...]: writes $gate_dir/NAME.json, always from a clean artifact environment.
run_gate() {
  local name="$1"
  shift
  set +e
  env -u MODEL_PLANE_CANDIDATE_ARTIFACT_DIR -u MODEL_PLANE_ROLLBACK_ARTIFACT_DIR \
    -u MODEL_PLANE_ARTIFACT_VERIFY_KEY -u MODEL_PLANE_ROLLBACK_ARTIFACT_VERIFY_KEY \
    -u CORESYSTEM_CONFORMANCE_PROFILE \
    MODEL_PLANE_RELEASE_ARTIFACT_SCRIPT="$gate_dir/stub-release.sh" \
    MODEL_PLANE_RELEASE_GATE_WAIVERS_FILE="$gate_dir/waivers.tsv" \
    CORESYSTEM_ROLLBACK_EVIDENCE_DIR="$gate_dir/evidence" \
    CORESYSTEM_CONFORMANCE_TODAY="$today" \
    "$@" bash "$GATE" --json --profile runtime >"$gate_dir/$name.json" 2>"$gate_dir/$name.err"
  local rc=$?
  set -e
  [[ "$rc" -eq 2 || "$rc" -eq 0 ]] || {
    echo "gate case $name: unexpected status $rc" >&2
    cat "$gate_dir/$name.err" >&2
    exit 1
  }
}

# expect_gate NAME 'python expression over r (the report)'
expect_gate() {
  python3 - "$gate_dir/$1.json" "$2" "$1" <<'PY'
import json
import sys

r = json.load(open(sys.argv[1]))
if not eval(sys.argv[2]):
    raise SystemExit(f"gate case {sys.argv[3]}: expectation failed: {sys.argv[2]}\n{json.dumps(r, indent=1)}")
PY
}

printf '# header only\n' >"$gate_dir/waivers.tsv"

# No artifacts and no evidence: the real gaps are named, the preflights are skipped.
run_gate nothing
expect_gate nothing '{"candidate_artifact_missing", "rollback_artifact_missing", "rollback_rehearsal_missing"} <= set(r["blockers"])'
expect_gate nothing 'r["profile"] == "runtime" and r["checks"]["dev_runtime_preflight"] == "skipped" and r["checks"]["cross_plane_preflight"] == "skipped"'
expect_gate nothing 'r["candidate"] == "absent" and r["waived"] == []'

# Verified artifacts: the candidate is verified in release mode against the configured key.
run_gate verified MODEL_PLANE_CANDIDATE_ARTIFACT_DIR="$gate_dir/good-candidate" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$gate_dir/good-rollback" MODEL_PLANE_ARTIFACT_VERIFY_KEY=trusted-key-ref
expect_gate verified 'r["candidate"] == "verified" and r["rollback"] == "verified"'
expect_gate verified 'not {"candidate_artifact_missing", "rollback_artifact_missing", "candidate_artifact_unverified", "rollback_artifact_unverified"} & set(r["blockers"])'
grep -q '^good-candidate mode=1 key=trusted-key-ref$' "$verify_log" || {
  echo "the candidate must be verified in release mode against the configured key" >&2
  exit 1
}

# A markdown claim is not evidence: a failed verification blocks, and cannot be waived.
run_gate unverified MODEL_PLANE_CANDIDATE_ARTIFACT_DIR="$gate_dir/bad-artifact" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$gate_dir/bad-artifact" MODEL_PLANE_ARTIFACT_VERIFY_KEY=trusted-key-ref
expect_gate unverified '{"candidate_artifact_unverified", "rollback_artifact_unverified"} <= set(r["blockers"])'
printf 'candidate_artifact_unverified\truntime\tops\tnot ready\t2026-10-20\t2026-10-30\tticket\n' >"$gate_dir/waivers.tsv"
run_gate unwaivable MODEL_PLANE_CANDIDATE_ARTIFACT_DIR="$gate_dir/bad-artifact" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$gate_dir/good-rollback" MODEL_PLANE_ARTIFACT_VERIFY_KEY=trusted-key-ref
expect_gate unwaivable '"candidate_artifact_unverified" in r["blockers"] and "release_gate_waivers_invalid" in r["blockers"]'
printf '# header only\n' >"$gate_dir/waivers.tsv"

# Signer anchor missing: nothing is verified against the artifact's own embedded key.
run_gate nokey MODEL_PLANE_CANDIDATE_ARTIFACT_DIR="$gate_dir/good-candidate" \
  MODEL_PLANE_ROLLBACK_ARTIFACT_DIR="$gate_dir/good-rollback"
expect_gate nokey '"artifact_verify_key_missing" in r["blockers"] and r["candidate"] == "unverified"'

# Rehearsal evidence: newest file wins; it must say PASSED and be younger than the limit.
printf '# Rollback rehearsal: result FAILED\n' >"$gate_dir/evidence/rollback-rehearsal-2026-10-22.txt"
run_gate rehearsal_failed
expect_gate rehearsal_failed '"rollback_rehearsal_failed" in r["blockers"] and r["checks"]["rollback_rehearsal"] == "failed"'
printf '# Rollback rehearsal: result PASSED\n' >"$gate_dir/evidence/rollback-rehearsal-2026-08-01.txt"
rm "$gate_dir/evidence/rollback-rehearsal-2026-10-22.txt"
run_gate rehearsal_stale
expect_gate rehearsal_stale '"rollback_rehearsal_stale" in r["blockers"]'
printf '# Rollback rehearsal: result PASSED\n' >"$gate_dir/evidence/rollback-rehearsal-2026-10-20.txt"
run_gate rehearsal_passed
expect_gate rehearsal_passed 'r["checks"]["rollback_rehearsal"] == "passed" and not any(b.startswith("rollback_rehearsal_") for b in r["blockers"])'

# Waivers: unexpired rows remove the blocker and are listed; expired rows do not; a bad row voids the file.
rm "$gate_dir/evidence/rollback-rehearsal-2026-10-20.txt" "$gate_dir/evidence/rollback-rehearsal-2026-08-01.txt"
printf 'rollback_rehearsal_missing\truntime\tops\tfirst host not ready\t2026-10-20\t2026-11-10\tticket-1\n' >"$gate_dir/waivers.tsv"
run_gate waived
expect_gate waived '"rollback_rehearsal_missing" not in r["blockers"] and r["waived"] == ["rollback_rehearsal_missing=2026-11-10"]'
printf 'rollback_rehearsal_missing\truntime\tops\tfirst host not ready\t2026-09-01\t2026-10-01\tticket-1\n' >"$gate_dir/waivers.tsv"
run_gate expired
expect_gate expired '{"rollback_rehearsal_missing", "waiver_expired:rollback_rehearsal_missing"} <= set(r["blockers"]) and r["waived"] == []'
printf 'rollback_rehearsal_missing\tfull\tops\tother profile\t2026-10-20\t2026-11-10\tticket-1\n' >"$gate_dir/waivers.tsv"
run_gate other_profile
expect_gate other_profile '"rollback_rehearsal_missing" in r["blockers"] and r["waived"] == []'
printf 'rollback_rehearsal_missing\truntime\tops\ttoo long\t2026-10-20\t2026-12-31\tticket-1\n' >"$gate_dir/waivers.tsv"
run_gate too_long
expect_gate too_long '"release_gate_waivers_invalid" in r["blockers"] and "rollback_rehearsal_missing" in r["blockers"] and r["waived"] == []'
# A valid row before a bad row must not be applied: the whole file is void.
printf 'rollback_rehearsal_missing\truntime\tops\tfirst host not ready\t2026-10-20\t2026-11-10\tticket-1\nbroken\n' >"$gate_dir/waivers.tsv"
run_gate void_file
expect_gate void_file '"release_gate_waivers_invalid" in r["blockers"] and "rollback_rehearsal_missing" in r["blockers"] and r["waived"] == []'

set +e
bash "$GATE" --profile nonsense >/dev/null 2>&1
profile_status=$?
set -e
[[ "$profile_status" -eq 64 ]] || {
  echo "an unknown profile must be refused (status 64), got $profile_status" >&2
  exit 1
}

echo "CoreSystem conformance artifact contract: ok"
