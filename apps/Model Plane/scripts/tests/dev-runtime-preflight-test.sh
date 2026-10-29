#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

if "$ROOT_DIR/scripts/tests/dev-runtime-preflight.sh" --help >"$TMP_DIR/help" 2>&1; then
  grep -F 'No credential values are printed or changed.' "$TMP_DIR/help" >/dev/null
else
  echo 'preflight help unexpectedly failed' >&2
  exit 1
fi

set +e
output="$("$ROOT_DIR/scripts/tests/dev-runtime-preflight.sh" 2>&1)"
exit_code=$?
set -e

# The current dev stack is intentionally unprovisioned for scheduled Control
# credentials. This contract test proves the preflight reports blockers and
# does not echo any credential assignment value.
if [[ "$exit_code" -ne 2 ]]; then
  echo "preflight expected blocked status 2, got $exit_code" >&2
  printf '%s\n' "$output" >&2
  exit 1
fi
grep -F 'STATUS blocked; scheduled effects remain fail-closed' <<<"$output" >/dev/null
grep -F 'CAPABILITY_CORE_CONTROL_SCHEDULE_SERVICE_TOKEN' <<<"$output" >/dev/null
grep -F 'ORCHESTRATOR_CORE_CONTROL_SCHEDULE_STEP_SERVICE_TOKEN' <<<"$output" >/dev/null
if grep -Eq '^[A-Z][A-Z0-9_]*=[^[:space:]]+' <<<"$output"; then
  echo 'preflight output appears to contain a configuration value' >&2
  printf '%s\n' "$output" >&2
  exit 1
fi

printf 'dev runtime preflight contracts: ok\n'
