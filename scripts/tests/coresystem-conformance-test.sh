#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
COMMAND="$ROOT_DIR/scripts/coresystem-conformance.sh"

[[ -x "$COMMAND" ]] || {
  echo "missing executable: $COMMAND" >&2
  exit 1
}

help_output="$($COMMAND --help)"
grep -q "read-only" <<<"$help_output"
grep -q "credential" <<<"$help_output"
grep -q "conformance-artifact.sh" <<<"$help_output"

set +e
human_output="$($COMMAND 2>&1)"
human_status=$?
json_output="$($COMMAND --json 2>&1)"
json_status=$?
set -e

[[ "$human_status" -eq 2 ]] || {
  echo "expected current dev conformance to be blocked (status 2), got $human_status" >&2
  printf '%s\n' "$human_output" >&2
  exit 1
}
[[ "$json_status" -eq 2 ]] || {
  echo "expected current dev JSON conformance to be blocked (status 2), got $json_status" >&2
  printf '%s\n' "$json_output" >&2
  exit 1
}

grep -q "CoreSystem conformance" <<<"$human_output"
grep -q "R-0" <<<"$human_output"
grep -q "STATUS blocked" <<<"$human_output"
grep -q '"status":"blocked"' <<<"$json_output"
grep -q '"report_version":"1"' <<<"$json_output"
grep -q '"source_revision"' <<<"$json_output"
grep -q '"cross_plane_preflight":"blocked"' <<<"$json_output"
grep -q '"cross_plane_preflight"' <<<"$json_output"
grep -q '"capability_promotion_ledger":"source_only"' <<<"$json_output"
grep -q '"capability_promotion_unproven"' <<<"$json_output"

# Conformance output may expose names and states, never secret-shaped values.
if grep -Eq '(TOKEN|SECRET|PRIVATE_KEY|BASE64)[A-Z_]*=[^[:space:]]+' <<<"$human_output$json_output"; then
  echo "conformance output leaked a secret-shaped assignment" >&2
  exit 1
fi
if grep -Eq '"(TOKEN|SECRET|PRIVATE_KEY|BASE64)[A-Z_]*":"[^"]+"' <<<"$json_output"; then
  echo "conformance JSON leaked a secret-shaped value" >&2
  exit 1
fi

echo "CoreSystem conformance contracts: ok"
